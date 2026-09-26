/** Bottom-center toasts, same look as the prototype. */

/** Shows `msg` for a few seconds; `kind: "error"` paints it red. */
export function toast(msg: string, kind?: "error"): void {
  const host = document.getElementById("toasts");
  if (!host) return;
  const el = document.createElement("div");
  el.className = kind ? `toast ${kind}` : "toast";
  el.textContent = msg;
  host.append(el);
  setTimeout(() => el.remove(), 2800);
}
