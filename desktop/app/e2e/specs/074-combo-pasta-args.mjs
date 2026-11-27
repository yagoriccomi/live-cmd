// 020 (v0.2.4): Tab completa pasta com "\"; sugestão em sombra na caixa; seta para baixo entra na lista; args usa o mesmo componente.
export default async (h) => {
  await h.js(`newAi(LOCAL)`);
  await h.until(`return !document.getElementById("ai-scrim").hidden`, 5000);
  const type = (id, v) => h.js(`const c = document.getElementById(arguments[0]); c.focus(); c.value = arguments[1]; c.dispatchEvent(new Event("input"))`, id, v);
  await type("ai-cwd", String.raw`C:\Windows\Syste`);
  await h.until(`return document.querySelectorAll("#ai-ac .ac-item").length`, 8000);
  h.eq(await h.js(`return document.querySelector("#ai-cwd + .ac-ghost")?.textContent.slice(16) === document.querySelector("#ai-ac .ac-item")?.textContent.slice(16) && getComputedStyle(document.querySelector("#ai-cwd + .ac-ghost")).opacity < 0.6`), true, "sombra da sugestão");
  await h.key("\uE004"); // Tab real
  h.eq(await h.js(`return document.getElementById("ai-cwd").value`), "C:\\Windows\\System\\", "Tab completou com barra");
  await h.until(`return document.querySelectorAll("#ai-ac .ac-item").length`, 8000);
  await h.key("\uE015"); // seta para baixo
  h.eq(await h.js(`return document.querySelector("#ai-ac .ac-item.on")?.id === document.getElementById("ai-cwd").getAttribute("aria-activedescendant")`), true, "seta entrou na lista");
  await type("ai-args", "--mo");
  await h.until(`return document.querySelectorAll("#ai-ac2 .ac-item").length`, 5000);
  await h.key("\uE004");
  h.eq(await h.js(`return /^--model \\S+ $/.test(document.getElementById("ai-args").value)`), true, "args completou: " + await h.js(`return document.getElementById("ai-args").value`));
  await h.js(`document.getElementById("ai-cancel").click()`);
  await h.until(`return document.getElementById("ai-scrim").hidden`, 3000);
};
