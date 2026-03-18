//! Accessibility audit checks.

use std::cell::RefCell;
use std::rc::Rc;

use super::super::{Category, Finding, Fix, Scope, Severity};

/// Site-level accessibility checks (advisory).
pub fn site_checks() -> Vec<Finding> {
    vec![Finding {
        id: "a11y/color-contrast-hint",
        category: Category::Accessibility,
        severity: Severity::Low,
        scope: Scope::Site,
        message: "Remember to verify color contrast ratios meet WCAG AA (4.5:1 for text)."
            .into(),
        fix: Fix {
            file: "templates/**/*.html".into(),
            instruction: "Use a contrast checker to verify all text meets WCAG AA ratios."
                .into(),
        },
    }]
}

/// Page-level accessibility checks (HTML inspection via lol_html).
pub fn page_checks(html: &str, page_path: &str, template_path: &str) -> Vec<Finding> {
    let has_lang = Rc::new(RefCell::new(false));
    let has_viewport = Rc::new(RefCell::new(false));
    let imgs_missing_alt: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(Vec::new()));
    let empty_links: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(Vec::new()));
    let current_link_text: Rc<RefCell<String>> = Rc::new(RefCell::new(String::new()));
    let current_link_href: Rc<RefCell<String>> = Rc::new(RefCell::new(String::new()));
    let current_link_has_aria = Rc::new(RefCell::new(false));

    // Clone references for element handler closures.
    let has_lang_c = has_lang.clone();
    let has_viewport_c = has_viewport.clone();
    let imgs_c = imgs_missing_alt.clone();
    let link_text_c = current_link_text.clone();
    let link_href_c = current_link_href.clone();
    let link_aria_c = current_link_has_aria.clone();
    let empty_links_c = empty_links.clone();

    // Clone for text handler.
    let link_text_t = current_link_text.clone();

    let result = lol_html::rewrite_str(
        html,
        lol_html::RewriteStrSettings {
            element_content_handlers: vec![
                // Check <html lang="...">
                lol_html::element!("html", move |el| {
                    if el.get_attribute("lang").is_some() {
                        *has_lang_c.borrow_mut() = true;
                    }
                    Ok(())
                }),
                // Check <meta name="viewport" content="...width=device-width...">
                lol_html::element!("meta[name='viewport']", move |el| {
                    if let Some(content) = el.get_attribute("content") {
                        if content.contains("width=device-width") {
                            *has_viewport_c.borrow_mut() = true;
                        }
                    }
                    Ok(())
                }),
                // Check <img> for alt attribute
                lol_html::element!("img", move |el| {
                    if !el.has_attribute("alt") {
                        let src = el.get_attribute("src").unwrap_or_default();
                        imgs_c.borrow_mut().push(src);
                    }
                    Ok(())
                }),
                // Check <a href="..."> for empty text content
                lol_html::element!("a[href]", move |el| {
                    link_text_c.borrow_mut().clear();
                    let href = el.get_attribute("href").unwrap_or_default();
                    *link_href_c.borrow_mut() = href;
                    *link_aria_c.borrow_mut() = el.has_attribute("aria-label");

                    let text_end = current_link_text.clone();
                    let href_end = current_link_href.clone();
                    let aria_end = current_link_has_aria.clone();
                    let links_end = empty_links_c.clone();

                    if let Some(handlers) = el.end_tag_handlers() {
                        let handler: lol_html::EndTagHandler<'static> =
                            Box::new(move |_end| {
                                let content = text_end.borrow().trim().to_string();
                                if content.is_empty() && !*aria_end.borrow() {
                                    links_end
                                        .borrow_mut()
                                        .push(href_end.borrow().clone());
                                }
                                text_end.borrow_mut().clear();
                                Ok(())
                            });
                        handlers.push(handler);
                    }

                    Ok(())
                }),
                // Text handler to accumulate text inside <a>
                lol_html::text!("a", move |text| {
                    link_text_t.borrow_mut().push_str(text.as_str());
                    Ok(())
                }),
            ],
            ..lol_html::RewriteStrSettings::new()
        },
    );

    // If lol_html fails, return empty — don't crash the audit.
    if result.is_err() {
        return Vec::new();
    }

    let mut findings = Vec::new();

    if !*has_lang.borrow() {
        findings.push(Finding {
            id: "a11y/html-lang",
            category: Category::Accessibility,
            severity: Severity::Critical,
            scope: Scope::Page,
            message: format!("{page_path}: <html> element is missing the `lang` attribute."),
            fix: Fix {
                file: template_path.into(),
                instruction: "Add lang attribute: <html lang=\"en\">".into(),
            },
        });
    }

    if !*has_viewport.borrow() {
        findings.push(Finding {
            id: "a11y/viewport-meta",
            category: Category::Accessibility,
            severity: Severity::Critical,
            scope: Scope::Page,
            message: format!(
                "{page_path}: missing <meta name=\"viewport\"> with width=device-width."
            ),
            fix: Fix {
                file: template_path.into(),
                instruction: "Add to <head>: <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">".into(),
            },
        });
    }

    for src in imgs_missing_alt.borrow().iter() {
        findings.push(Finding {
            id: "a11y/img-alt-text",
            category: Category::Accessibility,
            severity: Severity::High,
            scope: Scope::Page,
            message: format!("{page_path}: <img src=\"{src}\"> is missing an `alt` attribute."),
            fix: Fix {
                file: template_path.into(),
                instruction: format!(
                    "Add alt text: <img src=\"{src}\" alt=\"descriptive text\">. \
                     Use alt=\"\" for decorative images."
                ),
            },
        });
    }

    for href in empty_links.borrow().iter() {
        findings.push(Finding {
            id: "a11y/link-text",
            category: Category::Accessibility,
            severity: Severity::Medium,
            scope: Scope::Page,
            message: format!(
                "{page_path}: <a href=\"{href}\"> has no accessible text content."
            ),
            fix: Fix {
                file: template_path.into(),
                instruction: "Add visible link text or an aria-label attribute.".into(),
            },
        });
    }

    findings
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_color_contrast_hint_always_emitted() {
        let findings = site_checks();
        assert!(findings.iter().any(|f| f.id == "a11y/color-contrast-hint"));
    }

    #[test]
    fn test_missing_html_lang() {
        let html = "<html><head></head><body></body></html>";
        let findings = page_checks(html, "/test.html", "templates/test.html");
        assert!(findings.iter().any(|f| f.id == "a11y/html-lang"));
    }

    #[test]
    fn test_has_html_lang() {
        let html = r#"<html lang="en"><head></head><body></body></html>"#;
        let findings = page_checks(html, "/test.html", "templates/test.html");
        assert!(!findings.iter().any(|f| f.id == "a11y/html-lang"));
    }

    #[test]
    fn test_missing_viewport() {
        let html = "<html><head></head><body></body></html>";
        let findings = page_checks(html, "/test.html", "templates/test.html");
        assert!(findings.iter().any(|f| f.id == "a11y/viewport-meta"));
    }

    #[test]
    fn test_has_viewport() {
        let html = r#"<html><head><meta name="viewport" content="width=device-width, initial-scale=1"></head><body></body></html>"#;
        let findings = page_checks(html, "/test.html", "templates/test.html");
        assert!(!findings.iter().any(|f| f.id == "a11y/viewport-meta"));
    }

    #[test]
    fn test_img_missing_alt() {
        let html = r#"<html><head></head><body><img src="photo.jpg"></body></html>"#;
        let findings = page_checks(html, "/test.html", "templates/test.html");
        assert!(findings.iter().any(|f| f.id == "a11y/img-alt-text"));
    }

    #[test]
    fn test_img_has_alt() {
        let html =
            r#"<html><head></head><body><img src="photo.jpg" alt="A photo"></body></html>"#;
        let findings = page_checks(html, "/test.html", "templates/test.html");
        assert!(!findings.iter().any(|f| f.id == "a11y/img-alt-text"));
    }

    #[test]
    fn test_img_empty_alt_decorative() {
        let html =
            r#"<html><head></head><body><img src="spacer.gif" alt=""></body></html>"#;
        let findings = page_checks(html, "/test.html", "templates/test.html");
        assert!(!findings.iter().any(|f| f.id == "a11y/img-alt-text"));
    }

    #[test]
    fn test_empty_link_text() {
        let html = r#"<html><head></head><body><a href="/foo"></a></body></html>"#;
        let findings = page_checks(html, "/test.html", "templates/test.html");
        assert!(findings.iter().any(|f| f.id == "a11y/link-text"));
    }

    #[test]
    fn test_link_with_text() {
        let html =
            r#"<html><head></head><body><a href="/foo">Click here</a></body></html>"#;
        let findings = page_checks(html, "/test.html", "templates/test.html");
        assert!(!findings.iter().any(|f| f.id == "a11y/link-text"));
    }

    #[test]
    fn test_link_with_aria_label_no_text() {
        let html = r#"<html><head></head><body><a href="/foo" aria-label="Home"></a></body></html>"#;
        let findings = page_checks(html, "/test.html", "templates/test.html");
        assert!(!findings.iter().any(|f| f.id == "a11y/link-text"));
    }

    #[test]
    fn test_viewport_missing_width_device_width() {
        let html = r#"<html><head><meta name="viewport" content="initial-scale=1"></head><body></body></html>"#;
        let findings = page_checks(html, "/test.html", "templates/test.html");
        assert!(
            findings.iter().any(|f| f.id == "a11y/viewport-meta"),
            "viewport without width=device-width should be flagged"
        );
    }

    #[test]
    fn test_multiple_imgs_some_missing_alt() {
        let html = r#"<html><head></head><body><img src="a.jpg" alt="ok"><img src="b.jpg"><img src="c.jpg" alt=""></body></html>"#;
        let findings = page_checks(html, "/test.html", "templates/test.html");
        let alt_findings: Vec<_> =
            findings.iter().filter(|f| f.id == "a11y/img-alt-text").collect();
        assert_eq!(alt_findings.len(), 1, "only b.jpg should be flagged");
    }
}
