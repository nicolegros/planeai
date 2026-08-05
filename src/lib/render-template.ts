/**
 * Render a template string by replacing `{varName}` or `{varName:transform}` placeholders
 * with values from the provided context object.
 *
 * Supported transforms:
 * - `:slug` — lowercase, replace non-word chars with hyphens, trim leading/trailing hyphens
 * - `:lower` — lowercase
 * - `:upper` — uppercase
 */
// eslint-disable-next-line @typescript-eslint/no-explicit-any
export function renderTemplate(template: string, context: Record<string, any>): string {
  return template.replace(/\{(\w+)(?::(\w+))?\}/g, (_, varName, transform) => {
    let val =
      varName === "blocked_by"
        ? (context.blocked_by as string[] | undefined)?.join(", ") ?? ""
        : String(context[varName] ?? "");
    if (varName === "parent_key" && !val) val = String(context.key ?? "");
    if (transform === "slug")
      return val
        .toLowerCase()
        .replace(/[^\w]+/g, "-")
        .replace(/^-|-$/g, "");
    if (transform === "lower") return val.toLowerCase();
    if (transform === "upper") return val.toUpperCase();
    return val;
  });
}
