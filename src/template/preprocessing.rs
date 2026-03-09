//! Step 4.2: Fragment marker injection.
//!
//! Before registering templates with minijinja, we preprocess them to inject
//! HTML comment markers around `{% block %}` definitions. These markers survive
//! rendering and are later used by the build engine to extract fragment HTML.
//!
//! Markers:
//! - `<!--FRAG:<name>:START-->` injected immediately after `{% block <name> %}`
//! - `<!--FRAG:<name>:END-->` injected immediately before `{% endblock %}`
//!
//! Handles:
//! - Named endblocks: `{% endblock content %}`
//! - Nested blocks (track depth)
//! - Whitespace variations in the tags

use regex::Regex;

/// Inject `<!--FRAG:name:START-->` / `<!--FRAG:name:END-->` markers around
/// every `{% block name %}...{% endblock %}` in the template source.
///
/// This allows the build engine to extract the rendered content of individual
/// blocks as fragments (for HTMX partial loading).
pub fn inject_fragment_markers(source: &str) -> String {
    // We use a simple state-machine approach rather than a single regex
    // replacement, because we need to track block nesting to pair each
    // {% endblock %} with the correct {% block %}.

    let block_open_re = Regex::new(r"\{%-?\s*block\s+(\w+)\s*-?%\}").unwrap();
    let block_close_re = Regex::new(r"\{%-?\s*endblock(?:\s+\w+)?\s*-?%\}").unwrap();

    // Collect all opening and closing block tags with their positions.
    let mut events: Vec<BlockEvent> = Vec::new();

    for cap in block_open_re.captures_iter(source) {
        let m = cap.get(0).unwrap();
        events.push(BlockEvent {
            pos: m.start(),
            end: m.end(),
            kind: BlockEventKind::Open {
                name: cap[1].to_string(),
            },
        });
    }

    for m in block_close_re.find_iter(source) {
        events.push(BlockEvent {
            pos: m.start(),
            end: m.end(),
            kind: BlockEventKind::Close,
        });
    }

    // Sort by position in the source.
    events.sort_by_key(|e| e.pos);

    // Walk through events, maintaining a stack of open block names.
    // We record insertions to make after each open tag and before each close tag.
    let mut insertions: Vec<Insertion> = Vec::new();
    let mut stack: Vec<String> = Vec::new();

    for event in &events {
        match &event.kind {
            BlockEventKind::Open { name } => {
                // Insert START marker after the opening tag.
                insertions.push(Insertion {
                    pos: event.end,
                    text: format!("<!--FRAG:{}:START-->", name),
                });
                stack.push(name.clone());
            }
            BlockEventKind::Close => {
                if let Some(name) = stack.pop() {
                    // Insert END marker before the closing tag.
                    insertions.push(Insertion {
                        pos: event.pos,
                        text: format!("<!--FRAG:{}:END-->", name),
                    });
                }
                // If stack is empty, this is an unmatched endblock — leave it alone.
            }
        }
    }

    // Apply insertions from the end of the string backwards so positions remain valid.
    insertions.sort_by(|a, b| b.pos.cmp(&a.pos));

    let mut result = source.to_string();
    for ins in &insertions {
        result.insert_str(ins.pos, &ins.text);
    }

    result
}

#[derive(Debug)]
struct BlockEvent {
    pos: usize,
    end: usize,
    kind: BlockEventKind,
}

#[derive(Debug)]
enum BlockEventKind {
    Open { name: String },
    Close,
}

#[derive(Debug)]
struct Insertion {
    pos: usize,
    text: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_single_block() {
        let src = "{% block content %}Hello{% endblock %}";
        let result = inject_fragment_markers(src);
        assert_eq!(
            result,
            "{% block content %}<!--FRAG:content:START-->Hello<!--FRAG:content:END-->{% endblock %}"
        );
    }

    #[test]
    fn test_multiple_blocks() {
        let src = "{% block title %}Page{% endblock %}\n{% block content %}Body{% endblock %}";
        let result = inject_fragment_markers(src);
        assert!(result.contains("<!--FRAG:title:START-->Page<!--FRAG:title:END-->"));
        assert!(result.contains("<!--FRAG:content:START-->Body<!--FRAG:content:END-->"));
    }

    #[test]
    fn test_nested_blocks() {
        let src = "{% block outer %}A{% block inner %}B{% endblock %}C{% endblock %}";
        let result = inject_fragment_markers(src);
        // Inner block markers.
        assert!(result.contains("<!--FRAG:inner:START-->B<!--FRAG:inner:END-->"));
        // Outer block markers wrap everything including the inner block.
        assert!(result.contains("<!--FRAG:outer:START-->"));
        assert!(result.contains("<!--FRAG:outer:END-->{% endblock %}"));
    }

    #[test]
    fn test_named_endblock() {
        let src = "{% block content %}Hello{% endblock content %}";
        let result = inject_fragment_markers(src);
        assert_eq!(
            result,
            "{% block content %}<!--FRAG:content:START-->Hello<!--FRAG:content:END-->{% endblock content %}"
        );
    }

    #[test]
    fn test_whitespace_variations() {
        let src = "{%  block  content  %}Hello{%  endblock  %}";
        let result = inject_fragment_markers(src);
        assert!(result.contains("<!--FRAG:content:START-->Hello<!--FRAG:content:END-->"));
    }

    #[test]
    fn test_trim_whitespace_tags() {
        let src = "{%- block content -%}Hello{%- endblock -%}";
        let result = inject_fragment_markers(src);
        assert!(result.contains("<!--FRAG:content:START-->Hello<!--FRAG:content:END-->"));
    }

    #[test]
    fn test_no_blocks() {
        let src = "<h1>Hello World</h1>";
        let result = inject_fragment_markers(src);
        assert_eq!(result, src);
    }

    #[test]
    fn test_block_in_extends_template() {
        let src = r#"{% extends "_base.html" %}
{% block title %}My Title{% endblock %}
{% block content %}
<h1>Page Content</h1>
{% endblock %}"#;
        let result = inject_fragment_markers(src);
        assert!(result.contains("<!--FRAG:title:START-->My Title<!--FRAG:title:END-->"));
        assert!(result.contains("<!--FRAG:content:START-->\n<h1>Page Content</h1>\n<!--FRAG:content:END-->"));
    }

    #[test]
    fn test_preserves_surrounding_content() {
        let src = "BEFORE{% block x %}MID{% endblock %}AFTER";
        let result = inject_fragment_markers(src);
        assert_eq!(
            result,
            "BEFORE{% block x %}<!--FRAG:x:START-->MID<!--FRAG:x:END-->{% endblock %}AFTER"
        );
    }
}
