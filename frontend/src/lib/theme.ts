/** Apply SuperTask light/dark class on <html>. Unknown values fall back to light. */
export function applyTheme(theme: string | null | undefined) {
  const dark = theme === "dark";
  document.documentElement.classList.toggle("dark", dark);
  document.documentElement.style.colorScheme = dark ? "dark" : "light";
}
