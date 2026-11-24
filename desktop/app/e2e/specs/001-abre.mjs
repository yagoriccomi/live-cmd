// O app abre a janela do terminal com o script carregado (sem erro no carregamento).
export default async (h) => {
  await h.until(`return typeof newSession === "function" && document.getElementById("local-ul") !== null`);
};
