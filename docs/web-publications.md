# Readium Web Publications (`.webpub`)

Bokheim uses the Readium Packaging Format for source-preserving HTML
publications and future offline article captures. A Web Publication is a ZIP
container with the standard media type `application/webpub+zip`, the `.webpub`
extension, and a Readium `manifest.json` in its root.

The HTML, CSS, images, fonts, and media keep their source bytes and relative
archive paths. Packaging is not an EPUB conversion.

## Manifest

The manifest follows the Readium Web Publication Manifest model:

```json
{
  "@context": "https://readium.org/webpub-manifest/context.jsonld",
  "metadata": {
    "@type": "https://schema.org/Article",
    "title": "Article title",
    "author": ["Author Name"],
    "publisher": "Publication",
    "published": "2026-08-02",
    "language": "en"
  },
  "links": [
    { "rel": "self", "href": "manifest.json", "type": "application/webpub+json" },
    { "rel": "canonical", "href": "https://example.com/article" }
  ],
  "readingOrder": [
    { "href": "article.html", "type": "text/html", "title": "Article title" }
  ],
  "resources": [
    { "href": "styles/article.css", "type": "text/css" },
    { "href": "images/lead.jpg", "type": "image/jpeg" }
  ]
}
```

Article signals produce the Schema.org `Article` type; generic imported HTML
uses `CreativeWork`. Bokheim currently extracts title, author, publisher/site,
publication date, language, description, and canonical URL from common HTML,
Open Graph, and article metadata.

## Import policy

The filesystem-backed “Add book” flow traverses the dependency graph starting
at the selected HTML document. It includes only referenced local resources,
does not copy unrelated sibling files, does not fetch network resources, and
cannot leave the entry document's parent directory. Every packaged file is
declared in either `readingOrder` or `resources`.

Loose HTML remains available for direct opening. On platforms whose document
picker exposes only one file, a prebuilt `.webpub` is required when sidecar
assets must be included.

## Reader and validation policy

The native reader serves resources directly from the ZIP. MobileApp resolves
archive resources to temporary blob URLs and revokes them when the publication
closes. Both readers support multiple HTML documents in `readingOrder`.

Validation rejects encrypted archives, unsafe or duplicate paths, missing and
undeclared resources, invalid Readium contexts, missing titles, empty reading
orders, unsupported reading-order media, and size-limit violations. Active HTML
content and external active-resource URLs are not presented by MobileApp.

Readium specification: <https://readium.org/webpub-manifest/>
