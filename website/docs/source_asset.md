# Authenticated Source Assets

Download images and other assets from authenticated data sources using the
`source_asset()` template function.

## Problem

Data sources (CMSes, APIs) often return JSON containing image URLs that require
the same authentication as the API itself. Standard `<img>` tags can't send
auth headers, and eigen's asset localization doesn't know which source an image
belongs to.

## Usage

```jinja
{# Relative path — resolved against the source's base URL #}
<img src="{{ source_asset('my_cms', '/uploads/' ~ item.image.hash) }}">

{# Absolute URL from data #}
<img src="{{ source_asset('my_cms', item.image_url) }}">
```

### Arguments

| Argument | Type | Description |
|---|---|---|
| `source_name` | string | Must match a `[sources.*]` key in `site.toml` |
| `url_or_path` | string | Absolute URL or path relative to source base URL |

### URL Resolution

- Starts with `http://` or `https://` → used as-is (absolute)
- Otherwise → joined with the source's configured `url`

### Return Value

- **Build time:** Downloads the asset with the source's auth headers, saves to
  `dist/assets/`, and returns the local `/assets/...` path.
- **Dev time:** Returns a `/_proxy/{source_name}/...` URL. The dev server's
  proxy forwards the request with auth headers — no download needed.

## Configuration

No new configuration. Uses your existing `[sources.*]` setup:

```toml
[sources.my_cms]
url = "https://cms.example.com"
headers = { Authorization = "Bearer ${CMS_TOKEN}" }
```

## Error Handling

| Condition | Behavior |
|---|---|
| Unknown source name | Template render error listing available sources |
| Empty URL | Template render error |
| Download fails (build) | Warning logged, original URL left in place |
| Proxy fails (dev) | 502 Bad Gateway from dev proxy |

## Examples

### Strapi CMS with image hashes

```toml
# site.toml
[sources.strapi]
url = "https://strapi.mysite.com"
headers = { Authorization = "Bearer ${STRAPI_TOKEN}" }
```

```jinja
{# In your template #}
{% for post in posts %}
  <article>
    <img src="{{ source_asset('strapi', '/uploads/' ~ post.cover.hash ~ post.cover.ext) }}">
    <h2>{{ post.title }}</h2>
  </article>
{% endfor %}
```

### API with images on a different CDN

When images are hosted on a different domain but use the same auth:

```jinja
{# The API returns full URLs like https://media.example.com/photo.jpg #}
<img src="{{ source_asset('my_api', item.photo_url) }}">
```

In dev mode this proxies through `/_proxy/my_api/__source_asset__/https://media.example.com/photo.jpg`,
forwarding your API's auth headers. At build time, the image is downloaded with
auth and saved locally.
