# Modo de demonstração (só para captura de tela)

Renderiza as abas do app com os dados dos mockups V1 Glass Aurora (`docs/design/aurora-v1/`), sem rede e sem PC.
O `App.tsx` não importa nada desta pasta; ela só existe para a comparação pixel a pixel.

- `DemoApp.tsx`: `DemoScreen({ name })` com uma conexão falsa (fixtures). Nomes: `terminal`, `pareamento`, `erro`,
  `agentes`, `ssh`, `historico`.
- `index.ts`: entrada alternativa para o aparelho. Para capturar no Android, troque **localmente** o `main` do
  `package.json` para `demo/index.ts`, rode `npx expo start --dev-client`, abra o dev client e troque de tela com
  `adb shell am start -d "remoteterm://demo?s=agentes"`. Depois volte o `main` para `expo/AppEntry`.

## Comparação usada no PR

Web (react-native-web 0.21 + react-dom 19.3, montados com esbuild fora do repositório), Chrome headless em 390×844,
diferença com pixelmatch (`threshold: 0.1`). Resultados em `docs/design/aurora-v1/compare-mobile/`:
`<mockup>--compare.png` (mockup | app | diff) e `app-<tela>.png`.
