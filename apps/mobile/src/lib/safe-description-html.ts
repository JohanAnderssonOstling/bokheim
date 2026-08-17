const ALLOWED_ELEMENTS = new Set([
  "abbr", "b", "blockquote", "br", "cite", "code", "dd", "del", "dfn", "div", "dl", "dt",
  "em", "h1", "h2", "h3", "h4", "h5", "h6", "hr", "i", "ins", "kbd", "li", "mark",
  "ol", "p", "pre", "q", "s", "samp", "small", "span", "strike", "strong", "sub", "sup",
  "time", "tt", "u", "ul", "var", "wbr",
]);

const DROP_WITH_CONTENT = new Set([
  "base", "button", "embed", "form", "iframe", "input", "link", "math", "meta", "noscript", "object",
  "option", "script", "select", "style", "svg", "template", "textarea",
]);

/**
 * Keeps the structural text markup used by EPUB and catalogue descriptions,
 * while dropping executable, interactive, embedded, and attributed content.
 */
export function safeDescriptionHtml(value: string): string {
  const source = value.trim();
  if (!source) return "";

  const document = new DOMParser().parseFromString(source, "text/html");
  const elements = Array.from(document.body.querySelectorAll("*")).reverse();
  for (const element of elements) {
    const name = element.localName.toLowerCase();
    if (DROP_WITH_CONTENT.has(name)) {
      element.remove();
    } else if (!ALLOWED_ELEMENTS.has(name)) {
      element.replaceWith(...Array.from(element.childNodes));
    } else {
      for (const attribute of Array.from(element.attributes)) element.removeAttribute(attribute.name);
    }
  }
  if (!document.body.children.length) {
    return document.body.innerHTML.replace(/\r\n?|\n/g, "<br>");
  }
  return document.body.innerHTML;
}
