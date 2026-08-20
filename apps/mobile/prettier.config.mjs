/** @type {import("prettier").Config} */
export default {
  plugins: ["prettier-plugin-svelte"],
  printWidth: 90,
  singleAttributePerLine: true,
  singleQuote: false,
  trailingComma: "all",
  svelteSortOrder: "scripts-markup-styles-options",
};
