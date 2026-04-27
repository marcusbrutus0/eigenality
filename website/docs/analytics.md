# Analytics

Eigen injects analytics tracking snippets into every full rendered page before
`</body>`. Fragment files are not affected. Configure one or both providers
under `[analytics]` in `site.toml`.

## Google Analytics

```toml
[analytics.google]
tracking_id = "G-XXXXXXXXXX"
```

| Field | Required | Description |
|-------|----------|-------------|
| `tracking_id` | yes | Google Analytics measurement ID (e.g. `G-XXXXXXXXXX`) |

Injects the standard gtag.js async snippet.

## Umami

```toml
[analytics.umami]
website_id = "abc-123-def"
host_url = "https://analytics.example.com"
domains = "example.com,www.example.com"
auto_track = true
tag = "production"
```

| Field | Required | Default | Description |
|-------|----------|---------|-------------|
| `website_id` | yes | — | Website ID from the Umami dashboard |
| `host_url` | no | `https://cloud.umami.is` | Base URL of your Umami instance |
| `domains` | no | — | Comma-separated domains to restrict tracking to |
| `auto_track` | no | `true` | Automatically track page views |
| `tag` | no | — | Custom event tag applied to all events |

Injects a deferred script tag loading `{host_url}/script.js` with the
configured `data-*` attributes.

## Using Both

Both providers can be active simultaneously:

```toml
[analytics.google]
tracking_id = "G-XXXXXXXXXX"

[analytics.umami]
website_id = "abc-123-def"
```

Google's snippet is injected first, followed by Umami's.

## Disabling

Omit the `[analytics]` section entirely, or remove both sub-tables, to
disable all analytics injection.
