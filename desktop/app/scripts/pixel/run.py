"""Captura mockups V1 Aurora e a UI real (com stub do Tauri) em 1440x900 e mede a diferença.

Uso: python run.py <pasta-trabalho> [tela ...]
Gera em <pasta-trabalho>/png: mock-<Tela>.png, app-<Tela>.png, diff-<Tela>.png e compare-<Tela>.png
(mockup | app | diff lado a lado). Imprime a % de pixels diferentes (algum canal com diferença > 32).
"""
import pathlib, shutil, subprocess, sys
from PIL import Image, ImageChops

HERE = pathlib.Path(__file__).resolve().parent
UI = HERE.parents[1] / "ui"
SCREENS = {  # tela do mockup -> (página do app, hash do stub)
    "Terminal": ("terminal.html", "#terminal"), "Agentes": ("terminal.html", "#agentes"), "SSH": ("terminal.html", "#ssh"),
    "Historico": ("terminal.html", "#historico"), "Estados": ("terminal.html", "#estados"),
    "Config": ("terminal.html", "#settings"), "Pareamento": ("terminal.html", "#pair"),
}
THRESH = 32


def shot(page: pathlib.Path, out: pathlib.Path, hsh: str = "") -> None:
    subprocess.run(["powershell", "-NoProfile", "-ExecutionPolicy", "Bypass", "-File", str(HERE / "shot.ps1"), str(page), str(out), hsh], check=True)


def prep_app(work: pathlib.Path) -> pathlib.Path:
    app = work / "app"
    shutil.rmtree(app, ignore_errors=True)
    shutil.copytree(UI, app)
    shutil.copy(HERE / "stub.js", app / "stub.js")
    for name in ("terminal.html",):
        p = app / name
        p.write_text(p.read_text(encoding="utf-8").replace("<head>", '<head>\n<script src="stub.js"></script>', 1), encoding="utf-8")
    return app


def diff(a: pathlib.Path, b: pathlib.Path, out: pathlib.Path, cmp_out: pathlib.Path) -> float:
    ia, ib = Image.open(a).convert("RGB"), Image.open(b).convert("RGB").resize(Image.open(a).size)
    d = ImageChops.difference(ia, ib)
    mask = d.point(lambda v: 255 if v > THRESH else 0).convert("L").point(lambda v: 255 if v else 0)
    # Máscara por pixel: qualquer canal acima do limiar.
    r, g, bb = d.split()
    m = ImageChops.lighter(ImageChops.lighter(r, g), bb).point(lambda v: 255 if v > THRESH else 0)
    pct = 100.0 * sum(1 for v in m.getdata() if v) / (ia.width * ia.height)
    vis = Image.blend(ia.convert("L").convert("RGB"), Image.new("RGB", ia.size, (0, 0, 0)), 0.6)
    vis.paste(Image.new("RGB", ia.size, (255, 40, 120)), mask=m)
    vis.save(out)
    w, h = ia.size
    sbs = Image.new("RGB", (w * 3, h))
    for i, im in enumerate((ia, ib, vis)):
        sbs.paste(im, (w * i, 0))
    sbs.save(cmp_out)
    del mask
    return pct


def main() -> None:
    work = pathlib.Path(sys.argv[1])
    names = sys.argv[2:] or list(SCREENS)
    png = work / "png"
    png.mkdir(parents=True, exist_ok=True)
    subprocess.run([sys.executable, str(HERE / "prep_mockups.py"), str(work / "mock")], check=True, capture_output=True)
    app = prep_app(work)
    for n in names:
        page, hsh = SCREENS[n]
        shot(work / "mock" / f"VD-{n}-1.html", png / f"mock-{n}.png")
        shot(app / page, png / f"app-{n}.png", hsh)
        pct = diff(png / f"mock-{n}.png", png / f"app-{n}.png", png / f"diff-{n}.png", png / f"compare-{n}.png")
        print(f"{n}: {pct:.2f}%")


if __name__ == "__main__":
    main()
