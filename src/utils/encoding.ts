export function safeEncodeURIComponent(value: string): string {
  return encodeURIComponent(value.replace(
    /[\uD800-\uDBFF][\uDC00-\uDFFF]|[\uD800-\uDFFF]/g,
    (surrogate) => surrogate.length === 2 ? surrogate : '\uFFFD',
  ));
}
