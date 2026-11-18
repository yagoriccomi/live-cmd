"""Prepara os mockups V1 Aurora para renderizar sem o support.js (que não existe local).

Uso: python prep_mockups.py <pasta-saida>
Lê docs/design/aurora-v1/VD-*.dc.html, troca {{accent}} por #8b5cf6, tira o support.js e o
script do componente, troca o Google Fonts pelas fontes locais do app e desembrulha <x-dc>/<helmet>.
"""
import pathlib, re, sys

ROOT = pathlib.Path(__file__).resolve().parents[4]
SRC = ROOT / "docs" / "design" / "aurora-v1"
FONTS = (ROOT / "desktop" / "app" / "ui" / "fonts" / "fonts.css").as_uri()


def prep(text: str) -> str:
    text = text.replace("{{accent}}", "#8b5cf6")
    text = re.sub(r'<script src="\./support\.js"></script>', "", text)
    text = re.sub(r'<script type="text/x-dc".*?</script>', "", text, flags=re.S)
    text = re.sub(r'<link rel="stylesheet" href="https://fonts\.googleapis\.com[^"]*">', f'<link rel="stylesheet" href="{FONTS}">', text)
    for tag in ("x-dc", "helmet"):
        text = text.replace(f"<{tag}>", '<div style="display:contents">').replace(f"</{tag}>", "</div>")
    return text


def main() -> None:
    out = pathlib.Path(sys.argv[1])
    out.mkdir(parents=True, exist_ok=True)
    for f in sorted(SRC.glob("*.dc.html")):
        (out / f.name.replace(".dc.html", ".html")).write_text(prep(f.read_text(encoding="utf-8")), encoding="utf-8")
        print(f.name)


if __name__ == "__main__":
    main()
