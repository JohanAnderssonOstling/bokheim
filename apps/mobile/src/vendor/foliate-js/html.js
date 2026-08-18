const sanitize = doc => {
    // Imported books are content, not applications. Keep presentation markup
    // while removing active elements and inline event handlers.
    doc.querySelectorAll('script, iframe, frame, object, embed, base').forEach(el => el.remove())
    for (const el of doc.querySelectorAll('*')) for (const attr of [...el.attributes]) {
        if (/^on/i.test(attr.name)) el.removeAttribute(attr.name)
        if ((attr.name === 'href' || attr.name === 'src') && /^\s*javascript:/i.test(attr.value))
            el.removeAttribute(attr.name)
    }
}

const makePublication = (sources, name, ownedURLs = [], suppliedMetadata = {}) => {
    if (!Array.isArray(sources)) sources = [sources]
    const documents = sources.map((source, sectionIndex) => {
        const doc = new DOMParser().parseFromString(source, 'text/html')
        sanitize(doc)
        const headings = [...doc.querySelectorAll('h1, h2, h3')]
        headings.forEach((heading, index) => {
            if (!heading.id) heading.id = `foliate-html-heading-${sectionIndex}-${index}`
        })
        const serialized = '<!doctype html>' + doc.documentElement.outerHTML
        const blob = new Blob([serialized], { type: 'text/html' })
        return { doc, headings, serialized, blob, url: URL.createObjectURL(blob) }
    })
    const title = suppliedMetadata.title
        || documents[0]?.doc.querySelector('title')?.textContent?.trim() || name

    const book = {
        metadata: { ...suppliedMetadata, title },
        sections: documents.map(({ serialized, blob, url }, index) => ({
            id: String(index), load: () => url,
            createDocument: () => new DOMParser().parseFromString(serialized, 'text/html'), size: blob.size,
        })),
        toc: documents.flatMap(({ headings }, sectionIndex) => headings.map(heading => ({
            label: heading.textContent?.trim() || heading.id, href: `${sectionIndex}#${heading.id}`,
        }))),
        resolveHref: href => {
            const [section, fragment] = href.split('#')
            return { index: Number(section) || 0, anchor: fragment ? doc => doc.getElementById(fragment) : undefined }
        },
        splitTOCHref: href => {
            const [section, fragment] = href?.split('#') ?? []
            return [Number(section) || 0, fragment]
        },
        getTOCFragment: (doc, id) => doc.getElementById(id),
        isExternal: uri => /^\w+:/i.test(uri) && !uri.startsWith('blob:'),
        getCover: () => null,
        destroy: () => {
            documents.forEach(({ url }) => URL.revokeObjectURL(url))
            ownedURLs.forEach(url => URL.revokeObjectURL(url))
        },
    }
    return book
}

// Adapter for a standalone HTML document. This creates Foliate's publication
// interface directly; it does not wrap or convert the source into an EPUB.
export const makeHTML = async file => makePublication(await file.text(), file.name)

const externalURL = value => /^(?:[a-z][a-z0-9+.-]*:|\/\/)/i.test(value)

const resolveArchivePath = (base, value) => {
    const raw = value.trim()
    if (!raw || raw.startsWith('#') || externalURL(raw)) return null
    const withoutSuffix = raw.split(/[?#]/, 1)[0]
    let decoded
    try { decoded = decodeURIComponent(withoutSuffix) }
    catch { return null }
    const parts = [
        ...(decoded.startsWith('/') ? [] : base.split('/').slice(0, -1)),
        ...decoded.replaceAll('\\', '/').split('/'),
    ]
    const normalized = []
    for (const part of parts) {
        if (!part || part === '.') continue
        if (part === '..') {
            if (!normalized.length) return null
            normalized.pop()
        }
        else normalized.push(part)
    }
    return normalized.join('/')
}

const mimeType = name => ({
    css: 'text/css', svg: 'image/svg+xml', png: 'image/png', jpg: 'image/jpeg', jpeg: 'image/jpeg',
    gif: 'image/gif', webp: 'image/webp', avif: 'image/avif', bmp: 'image/bmp', ico: 'image/x-icon',
    woff: 'font/woff', woff2: 'font/woff2', ttf: 'font/ttf', otf: 'font/otf', mp3: 'audio/mpeg',
    m4a: 'audio/mp4', mp4: 'video/mp4', webm: 'video/webm', ogg: 'audio/ogg', wav: 'audio/wav',
})[name.split('.').pop()?.toLowerCase()] ?? 'application/octet-stream'

const rewriteCSS = async (source, base, materialize) => {
    const pattern = /url\(\s*(["']?)([^"')]+)\1\s*\)/gi
    let result = '', offset = 0, match
    while ((match = pattern.exec(source))) {
        result += source.slice(offset, match.index)
        const path = resolveArchivePath(base, match[2])
        const url = path ? await materialize(path) : null
        result += url ? `url("${url}")` : externalURL(match[2]) ? 'url("")' : match[0]
        offset = pattern.lastIndex
    }
    result += source.slice(offset)
    const imports = /@import\s+(["'])([^"']+)\1/gi
    source = result, result = '', offset = 0
    while ((match = imports.exec(source))) {
        result += source.slice(offset, match.index)
        const path = resolveArchivePath(base, match[2])
        const url = path ? await materialize(path) : null
        result += url ? `@import url("${url}")` : externalURL(match[2]) ? '@import url("")' : match[0]
        offset = imports.lastIndex
    }
    return result + source.slice(offset)
}

// Opens a packaged Readium Web Publication. Relative asset references are
// resolved to temporary blob URLs; archive contents themselves are untouched.
export const makeWebPub = async (loader, file) => {
    const manifestSource = await loader.loadText('manifest.json')
    if (!manifestSource) throw new Error('Web Publication manifest.json is missing')
    const manifest = JSON.parse(manifestSource)
    const contexts = Array.isArray(manifest['@context']) ? manifest['@context'] : [manifest['@context']]
    if (!contexts.includes('https://readium.org/webpub-manifest/context.jsonld')
        || !manifest.metadata?.title || !Array.isArray(manifest.readingOrder) || !manifest.readingOrder.length)
        throw new Error('Invalid Readium Web Publication manifest')
    const entries = manifest.readingOrder.map(link => {
        if (!['text/html', 'application/xhtml+xml'].includes(link?.type))
            throw new Error('Web Publication readingOrder contains unsupported content')
        const entry = resolveArchivePath('bundle-root', link.href)
        if (!entry || !/\.(?:html?|xhtml)$/i.test(entry)) throw new Error('Invalid Web Publication readingOrder')
        return entry
    })
    const sectionByEntry = new Map(entries.map((entry, index) => [entry, index]))

    const urls = new Map(), loading = new Set(), ownedURLs = []
    const materialize = async name => {
        if (urls.has(name)) return urls.get(name)
        if (loading.has(name)) return null
        loading.add(name)
        let blob
        if (/\.css$/i.test(name)) {
            const css = await loader.loadText(name)
            if (css == null) { loading.delete(name); return null }
            blob = new Blob([await rewriteCSS(css, name, materialize)], { type: 'text/css' })
        }
        else {
            blob = await loader.loadBlob(name, mimeType(name))
            if (!blob) { loading.delete(name); return null }
        }
        const url = URL.createObjectURL(blob)
        urls.set(name, url), ownedURLs.push(url), loading.delete(name)
        return url
    }

    const rewriteDocument = async entry => {
        const source = await loader.loadText(entry)
        if (source == null) throw new Error(`Web Publication resource is missing: ${entry}`)
        const doc = new DOMParser().parseFromString(source, 'text/html')
        sanitize(doc)
        const rewriteAttribute = async (element, attribute, active = true) => {
            const value = element.getAttribute(attribute)
            if (!value || value.startsWith('#')) return
            const path = resolveArchivePath(entry, value)
            const url = path ? await materialize(path) : null
            if (url) element.setAttribute(attribute, url)
            else if (active) element.removeAttribute(attribute)
        }
        for (const element of doc.querySelectorAll('[src]')) await rewriteAttribute(element, 'src')
        for (const element of doc.querySelectorAll('[poster]')) await rewriteAttribute(element, 'poster')
        for (const element of doc.querySelectorAll('link[href]')) {
            if (element.relList?.contains('stylesheet')) await rewriteAttribute(element, 'href')
        }
        for (const anchor of doc.querySelectorAll('a[href]')) {
            const href = anchor.getAttribute('href'), path = resolveArchivePath(entry, href)
            if (path && sectionByEntry.has(path)) {
                const fragment = href.includes('#') ? `#${href.split('#').slice(1).join('#')}` : ''
                anchor.setAttribute('href', `${sectionByEntry.get(path)}${fragment}`)
            }
        }
        for (const element of doc.querySelectorAll('[style]'))
            element.setAttribute('style', await rewriteCSS(element.getAttribute('style'), entry, materialize))
        for (const style of doc.querySelectorAll('style'))
            style.textContent = await rewriteCSS(style.textContent ?? '', entry, materialize)
        for (const element of doc.querySelectorAll('[srcset]')) {
            const candidates = []
            for (const candidate of element.getAttribute('srcset').split(',')) {
                const [value, ...descriptor] = candidate.trim().split(/\s+/)
                const path = resolveArchivePath(entry, value)
                const url = path ? await materialize(path) : null
                if (url) candidates.push([url, ...descriptor].join(' '))
            }
            if (candidates.length) element.setAttribute('srcset', candidates.join(', '))
            else element.removeAttribute('srcset')
        }
        return '<!doctype html>' + doc.documentElement.outerHTML
    }
    const sources = []
    for (const entry of entries) sources.push(await rewriteDocument(entry))
    return makePublication(sources, file.name, ownedURLs, manifest.metadata)
}
