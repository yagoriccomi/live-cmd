// #64 (019/020): autocomplete da pasta aparece ao digitar, lista o conteúdo sem "\" final e Tab completa.
export default async (h) => {
  await h.js(`newAi(LOCAL)`);
  await h.until(`return !document.getElementById("ai-scrim").hidden`, 5000);
  await h.js(`const c = document.getElementById("ai-cwd"); c.focus(); c.value = "C:\\\\Windows"; c.dispatchEvent(new Event("input"))`);
  await h.until(`return [...document.querySelectorAll("#ai-ac .ac-item")].some((x) => x.textContent.toLowerCase() === "c:\\\\windows\\\\system32")`, 8000);
  await h.js(`const c = document.getElementById("ai-cwd"); c.value = "C:\\\\Windows\\\\Syste"; c.dispatchEvent(new Event("input"))`);
  await h.until(`const l = [...document.querySelectorAll("#ai-ac .ac-item")]; return l.length && l[0].textContent.toLowerCase().startsWith("c:\\\\windows\\\\syste")`, 8000);
  await h.js(`document.getElementById("ai-cwd").dispatchEvent(new KeyboardEvent("keydown", { key: "Tab", bubbles: true, cancelable: true }))`);
  const v = await h.js(`return document.getElementById("ai-cwd").value`);
  if (!/^C:\\Windows\\System/i.test(v)) throw new Error("Tab não completou: " + v);
  await h.js(`document.getElementById("ai-cancel").click()`);
  await h.until(`return document.getElementById("ai-scrim").hidden`, 3000);
};
