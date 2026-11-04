#!/usr/bin/env python3
"""Convert an unpacked Drakensang Online client into the assets this client
loads (glTF models + PNG/KTX2 textures + maps + prepared audio + tables),
using DSO_Godot and the tools beside this script.

  convert_assets.py <export_win32> <out_dir> [<DSO_Godot>] [--replace] [--progress]
  convert_assets.py <export_win32> <assets> --database-only [--progress]

  <export_win32>  the client's unpacked data tree (models/, meshes/, maps/,
                  textures/, anims/, db/static.db4, ...): game-data/export_win32
                  once the account screen downloaded it (docs/game-data.md).
  <out_dir>       where the converted assets go (~13 GB). Keep it OUTSIDE any
                  git-tracked path: the game data is proprietary.
  <DSO_Godot>     a checkout of https://github.com/DSO-Reconstruction/DSO_Godot
                  (a release's converter/DSO_Godot, else cloned into
                  <out_dir>/.DSO_Godot when omitted).
  --replace       convert into <out_dir>.partial and put it in place of
                  <out_dir> only once complete (the client's own conversion).
  --database-only rerun only the steps that read static.db4 or maps/*.db4, over
                  an existing conversion (after the account server's arrived).
  --progress      also print "@progress <done> <total> <what>" lines for the
                  client's progress bar.
  --watch-stdin   stop, with the running step, once stdin closes (the client
                  holds it open while it waits for the conversion).

Steps, in this order: audio, the Crunch transcoder, models, the two player
skeletons, maps and their event layers, then what glTF cannot carry and the
client's own tables (tools/*.py), the authored level edits (dsor-levels), the
models' textures as KTX2 (dsor-ktx2), the font. Last, <out>/conversion.json
records what was converted (the client compares it with its game data).

Finds its helpers (docs/release.md): a release's converter/bin (dsor-levels,
dsor-ktx2, crn2dds, vgmstream/), else this repository's cargo builds, DSO_Godot's
build/crn2dds (built with a C++ compiler when missing) and vgmstream-cli on PATH
or DSOR_VGMSTREAM. Needs Python with Pillow and numpy. Logs go to <out>/logs/.
"""
import hashlib
import json
import os
import re
import shutil
import signal
import subprocess
import sys
import threading
import time
from pathlib import Path

TOOLS = Path(__file__).resolve().parent
# The repository, or a release's converter/ folder (tools/, bin/, DSO_Godot/,
# level-overrides/, vfx-overrides/, VERSION).
HOME = TOOLS.parent
BIN = HOME / "bin"
EXE = ".exe" if os.name == "nt" else ""
STAMP = "conversion.json"
# The command running now, for --watch-stdin.
RUNNING = []


class Failed(Exception):
    pass


def usage():
    print(__doc__.split("\n\n")[1])
    sys.exit(2)


def version():
    """What converted: a release's VERSION, else the repository's commit."""
    try:
        return (HOME / "VERSION").read_text(encoding="utf-8").strip()
    except OSError:
        pass
    try:
        head = subprocess.run(["git", "rev-parse", "--short", "HEAD"], cwd=HOME, capture_output=True, text=True, timeout=10)
        return "dev-" + head.stdout.strip() if head.returncode == 0 else "dev"
    except (OSError, subprocess.SubprocessError):
        return "dev"


def sha256(path):
    digest = hashlib.sha256()
    with open(path, "rb") as f:
        for block in iter(lambda: f.read(1 << 20), b""):
            digest.update(block)
    return digest.hexdigest()


def stamp(root):
    """What the conversion of <root> stands for (docs/release.md): the build,
    the static database and the account server's map bundle when the client
    installed one (crates/game-data server.rs: .dsor/server-maps.txt)."""
    build = Path(root).parent / "__root.txt"
    database = Path(root) / "db" / "static.db4"
    maps = Path(root).parent / ".dsor" / "server-maps.txt"
    return {
        "converter": version(),
        "build": build.read_text(encoding="utf-8").strip() if build.is_file() else None,
        "static_db": sha256(database) if database.is_file() else None,
        "maps": maps.read_text(encoding="utf-8").split("\t")[0].strip() if maps.is_file() else None,
    }


class Run:
    def __init__(self, root, out, godot, progress):
        self.root, self.out, self.godot, self.progress = root, out, godot, progress
        self.logs = out / "logs"
        self.done = 0
        self.total = 0
        self.env = dict(os.environ)

    def report(self, what, within=0.0):
        # In hundredths of the weights: a step's own progress shows too.
        if self.progress:
            print(f"@progress {int((self.done + within) * 100)} {self.total * 100} {what}", flush=True)

    def run(self, command, log, cwd=None, append=False, name=None):
        """One command, its output in logs/<log>; a failure ends the conversion."""
        self.logs.mkdir(parents=True, exist_ok=True)
        path = self.logs / log
        with open(path, "a" if append else "w", encoding="utf-8", errors="replace") as f:
            f.write(f"$ {' '.join(map(str, command))}\n")
            f.flush()
            process = subprocess.Popen([str(c) for c in command], stdin=subprocess.DEVNULL, stdout=f,
                                       stderr=subprocess.STDOUT, cwd=str(cwd or HOME), env=self.env,
                                       start_new_session=os.name != "nt")
            RUNNING.append(process)
            code = process.wait()
            RUNNING.remove(process)
        if code != 0:
            tail = path.read_text(encoding="utf-8", errors="replace").splitlines()[-25:]
            raise Failed(f"{name or Path(str(command[0])).name} failed (exit {code}); log: {path}\n" + "\n".join(tail))

    def python(self, script, *args, log, append=False, cwd=None):
        # Through run_tool.py: an embedded Python (a release's) puts no script's
        # folder on sys.path, and the tools import their neighbours.
        self.run([sys.executable, TOOLS / "run_tool.py", script, *args], log, cwd=cwd, append=append, name=Path(script).name)

    def tail(self, log, lines):
        try:
            for line in (self.logs / log).read_text(encoding="utf-8", errors="replace").splitlines()[-lines:]:
                print(line)
        except OSError:
            pass

    def steps(self, steps):
        self.total = sum(weight for _, weight, _ in steps)
        started = time.monotonic()
        current = []
        stop = threading.Event()

        def heartbeat():
            # Every 15 s the step again, with how far it is when its log says
            # so: the client's strip shows the time so far, and moves.
            while not stop.wait(15):
                if current:
                    what, weight = current[0]
                    self.report(what, weight * self.within(what))
        if self.progress:
            threading.Thread(target=heartbeat, daemon=True).start()
        for what, weight, step in steps:
            current[:] = [(what, weight)]
            self.report(what)
            print(f"[convert] {what}", flush=True)
            began = time.monotonic()
            step()
            print(f"[convert] {what}: {time.monotonic() - began:.0f} s", flush=True)
            self.done += weight
        stop.set()
        current.clear()
        self.report("done")
        print(f"[convert] done in {(time.monotonic() - started) / 60:.1f} min -> {self.out}", flush=True)

    def within(self, what):
        """How far a long step is, from its log: DSO_Godot's `[export] i/n`."""
        if what != "models":
            return 0.0
        try:
            with open(self.logs / "export_models.log", "rb") as f:
                f.seek(0, os.SEEK_END)
                f.seek(max(0, f.tell() - 4096))
                tail = f.read().decode("utf-8", errors="replace")
        except OSError:
            return 0.0
        counts = re.findall(r"\[export\] (\d+)/(\d+)", tail)
        return min(1.0, int(counts[-1][0]) / max(1, int(counts[-1][1]))) if counts else 0.0


def watch_stdin():
    """Ends the conversion and the running step's whole process tree (DSO_Godot
    exports in worker processes) once stdin closes."""
    def watch():
        # The descriptor, not sys.stdin: a thread blocked in sys.stdin's read
        # holds its lock, and the interpreter's shutdown then dies waiting for
        # it ("Fatal Python error: _enter_buffered_busy").
        try:
            while os.read(sys.stdin.fileno(), 4096):
                pass
        except (OSError, ValueError):
            pass
        for process in list(RUNNING):
            if os.name == "nt":
                subprocess.call(["taskkill", "/T", "/F", "/PID", str(process.pid)],
                                stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
            else:
                try:
                    os.killpg(process.pid, signal.SIGKILL)
                except OSError:
                    pass
        print("error: stopped: the client closed", file=sys.stderr, flush=True)
        os._exit(3)
    threading.Thread(target=watch, daemon=True).start()


def tool(name):
    """A Rust helper: a release's bin/, else built and run with cargo."""
    binary = BIN / (name + EXE)
    if binary.is_file():
        return [binary]
    package = {"dsor-levels": "dsor-level-data", "dsor-ktx2": "dsor-texture-pack"}[name]
    return ["cargo", "run", "--quiet", "--locked", "--release", "--manifest-path", HOME / "Cargo.toml",
            "-p", package, "--bin", name, "--"]


def crunch(godot, build=False):
    """The .crn transcoder: ~2,600 textures are Crunch-compressed. DSO_Godot
    looks for build/crn2dds without .exe, so on Windows it is always named
    through DSO_CRN2DDS."""
    for candidate in (os.environ.get("DSO_CRN2DDS"), BIN / ("crn2dds" + EXE), godot / "build" / ("crn2dds" + EXE)):
        if candidate and Path(candidate).is_file():
            return Path(candidate)
    if not build:
        return None
    subprocess.check_call(["sh", str(godot / "tools" / "build_crn2dds.sh")])
    return godot / "build" / ("crn2dds" + EXE)


def level_cache():
    """The level tool's working folder: a release's own; in the repository
    the level editor's (.level-editor), as before."""
    return ["--cache", HOME / ".level-cache"] if (BIN / ("dsor-levels" + EXE)).is_file() else []


def vgmstream():
    for candidate in (os.environ.get("DSOR_VGMSTREAM"), BIN / "vgmstream" / ("vgmstream-cli" + EXE),
                      HOME / ".local" / "vgmstream" / ("vgmstream-cli" + EXE), shutil.which("vgmstream-cli")):
        if candidate and Path(candidate).is_file():
            return Path(candidate)
    return None


def font(out):
    """Tahoma, the game's font: the system's copy (the conversion makes none)."""
    target = out / "fonts" / "tahoma.ttf"
    if target.is_file():
        return
    windows = Path(os.environ.get("WINDIR", r"C:\Windows")) / "Fonts" / "tahoma.ttf"
    # Last, a Linux release's: Wine's Tahoma (tools/release/package.py, LGPL-2.1).
    for candidate in (windows, Path("/System/Library/Fonts/Supplemental/Tahoma.ttf"),
                      Path("/usr/share/fonts/truetype/msttcorefonts/tahoma.ttf"), HOME / "fonts" / "tahoma.ttf"):
        if candidate.is_file():
            target.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(candidate, target)
            return
    print("warning: no tahoma.ttf found: copy it to fonts/tahoma.ttf in the assets", file=sys.stderr)


def steps(r, database_only=False):
    """Every step in order: (what, weight, reads the databases, run). The
    weights are the steps' rough share of the time, for the progress bar. With
    database_only, the steps that read static.db4 or maps/*.db4 (the account
    server's, docs/release.md) or what those make: character_tables is left
    out, its tables being made again by the selection screen and interface
    steps, as rerunning it would drop export_deco's sets from
    characters/anims.json (export_deco reruns and puts its own back)."""
    root, out, godot = r.root, r.out, r.godot
    db = root / "db" / "static.db4"
    post = lambda script, *args: lambda: r.python(TOOLS / script, *args, log="post.log", append=True, cwd=godot)
    levels = lambda *args: r.run([*tool("dsor-levels"), *args], "levels.log", append=True)

    def audio():
        decoder = vgmstream()
        if decoder:
            r.env["DSOR_VGMSTREAM"] = str(decoder)
        r.python(TOOLS / "export_audio.py", root, out, log="export_audio.log")
        r.tail("export_audio.log", 3)

    def transcoder():
        r.env["DSO_CRN2DDS"] = str(crunch(godot, build=True))

    def models():
        r.python(godot / "batch_export.py", "--root", root, "--out", out, "--skip", "uniskel", "uniskel_dwarf",
                 "--report", r.logs / "export_report.json", log="export_models.log", cwd=godot)
        r.tail("export_models.log", 12)

    def characters():
        for c in ("uniskel", "uniskel_dwarf"):
            model = root / "models" / "characters" / f"{c}.n3"
            if model.is_file():
                r.python(godot / "split_character.py", model, "--root", root, "--export-root", out,
                         log=f"split_{c}.log", cwd=godot)
                r.tail(f"split_{c}.log", 4)

    def maps():
        r.python(godot / "export_maps.py", "--root", root, "--out", out / "maps", log="export_maps.log", cwd=godot)
        r.tail("export_maps.log", 6)
        levels("events", root / "maps", out / "maps")

    def ktx2():
        r.run([*tool("dsor-ktx2"), root, out], "ktx2.log")
        r.tail("ktx2.log", 1)

    every = [
        ("audio", 5, False, audio),
        ("the Crunch transcoder", 1, False, transcoder),
        ("models", 8, False, models),
        ("player characters", 3, False, characters),
        ("maps", 4, False, maps),
        # What glTF cannot carry, and the client's own tables.
        ("skin fragments", 1, False, post("restore_skin_fragments.py", root, out, godot)),
        ("materials", 1, False, post("fix_materials.py", out)),
        ("emitters", 1, False, post("embed_emitters.py", out)),
        ("animators", 1, False, post("embed_animators.py", root / "models", out)),
        ("character render states", 1, False, post("character_render_states.py", out)),
        ("cube maps and layers", 2, False, post("embed_textures.py", root, out, godot)),
        ("render states", 2, False, post("fix_materials.py", out, "--n3", root / "models")),
        ("character colours", 1, False, post("fix_materials.py", out, "--character-colors")),
        ("dye masks", 2, False, post("embed_character_masks.py", root, out, godot)),
        ("animation loops", 1, False, post("close_loops.py", out)),
        ("effect clips", 1, False, post("fx_clip_starts.py", root, out, godot)),
        ("pivots", 1, False, post("restore_pivots.py", root / "models", out, godot)),
        ("character tables", 2, False, post("character_tables.py", root, out)),
        ("monsters", 1, True, post("export_monsters.py", root, out)),
        ("picking shapes", 1, True, post("export_picking.py", root, out)),
        ("combat texts", 1, True, post("export_combat_text.py", root, out, godot)),
        ("statistics", 1, True, post("export_stats.py", root, out)),
        ("the selection screen", 1, True, post("export_character_selection.py", db, out)),
        ("the systems windows", 2, True, post("systems_catalog.py", db, root, godot, out)),
        ("skills", 2, True, post("export_skills.py", root, out)),
        ("pets", 1, True, post("export_pets.py", root, out)),
        ("NPCs", 1, True, post("export_npcs.py", root, out)),
        ("exits", 1, True, post("export_exits.py", root, out)),
        ("ambience", 1, True, post("export_ambience.py", root, out)),
        ("decorations", 1, True, post("export_deco.py", root, out)),
        # After the skills and the decorations' animation sets (the Night
        # Walker's spells over static.db4's niwalk rows, DSOR_Server
        # tools/install_niwalk_content.py).
        ("the Night Walker's spells", 1, True, post("niwalk_visuals.py", out)),
        ("map audio", 2, True, post("export_audio_runtime.py", root, out)),
        # After anything that rewrites an effect model or skills/sequences.json.
        ("VFX edits", 1, True, post("vfx_apply.py", out)),
        ("interface windows", 3, True, post("export_ui.py", root, out, godot)),
        ("the trade window", 1, False, post("compose_trade_ui.py", out)),
        ("the debug map", 1, False, post("export_debug.py", out)),
        ("navigation meshes", 2, False, post("export_navmesh.py", root, out)),
        # From the game data's own quests.pbxml (the server's quests.xml is that
        # pack decoded, from an older build).
        ("quests", 1, True, post("quest_catalog.py", root, out, "--database", db, "--godot", godot)),
        # Authored edits live outside the generated tree: recompose the client
        # sidecars (from the NPC templates and the selection screen too) and
        # derived DB4 files from one effective map.
        ("level edits", 1, True, lambda: levels("apply", "--assets", out, "--source-root", root,
                                                "--overrides", HOME / "level-overrides", *level_cache())),
        ("level props", 1, True, lambda: levels("props", "--assets", out, "--source-root", root,
                                                "--static-db", db, *level_cache())),
        ("KTX2 textures", 6, False, ktx2),
        ("the font", 1, False, lambda: font(out)),
    ]
    return [(what, weight, run) for what, weight, database, run in every if database or not database_only]


def dso_godot(given, out):
    if given:
        return Path(given).resolve()
    bundled = HOME / "DSO_Godot"
    if (bundled / "export_model.py").is_file():
        return bundled
    clone = out / ".DSO_Godot"
    if not (clone / "export_model.py").is_file():
        subprocess.check_call(["git", "clone", "--depth", "1", "https://github.com/DSO-Reconstruction/DSO_Godot", str(clone)])
    return clone


def replace(partial, out):
    """Puts a finished conversion in place of the old one."""
    old = out.with_name(out.name + ".old")
    if old.exists():
        shutil.rmtree(old)
    if out.exists():
        out.rename(old)
    partial.rename(out)
    if old.exists():
        shutil.rmtree(old, ignore_errors=True)


def main(argv):
    flags = {a for a in argv if a.startswith("--")}
    args = [a for a in argv if not a.startswith("--")]
    unknown = flags - {"--replace", "--progress", "--database-only", "--watch-stdin"}
    if len(args) < 2 or unknown:
        usage()
    root = Path(args[0]).resolve()
    out = Path(args[1]).resolve()
    if not ((root / "models").is_dir() and (root / "maps").is_dir()):
        print(f"error: {root} does not look like an unpacked export_win32 tree", file=sys.stderr)
        return 1
    try:
        import PIL, numpy  # noqa: F401
    except ImportError:
        print("error: Python needs Pillow and numpy (pip install Pillow numpy)", file=sys.stderr)
        return 1
    # After those imports: on Windows anything that asks about stdin's handle
    # waits while a thread reads it.
    if "--watch-stdin" in flags:
        watch_stdin()
    database_only = "--database-only" in flags
    if database_only and not (out / STAMP).is_file() and not (out / "maps").is_dir():
        print(f"error: {out} holds no conversion to refresh", file=sys.stderr)
        return 1
    work = out.with_name(out.name + ".partial") if "--replace" in flags and not database_only else out
    if work != out and work.exists():
        shutil.rmtree(work)
    work.mkdir(parents=True, exist_ok=True)
    godot = dso_godot(args[2] if len(args) > 2 else None, work)
    r = Run(root, work, godot, "--progress" in flags)
    found = crunch(godot)
    if found:
        r.env["DSO_CRN2DDS"] = str(found)
    try:
        r.steps(steps(r, database_only))
    except (Failed, subprocess.CalledProcessError, OSError) as error:
        # The log's tail first, the one-line reason last (the client shows it).
        summary, *details = str(error).splitlines() or [""]
        print("\n".join(details), flush=True)
        print(f"error: {summary}", file=sys.stderr, flush=True)
        return 1
    finished = stamp(root)
    previous = {}
    if database_only:
        try:
            previous = json.loads((out / STAMP).read_text(encoding="utf-8"))
        except (OSError, ValueError):
            pass
    finished = {**previous, **finished, "finished": time.strftime("%Y-%m-%dT%H:%M:%S%z")}
    if database_only:
        finished["converter"] = previous.get("converter", finished["converter"])
    (work / STAMP).write_text(json.dumps(finished, indent=2) + "\n", encoding="utf-8")
    if work != out:
        replace(work, out)
    return 0


if __name__ == "__main__":
    try:
        code = main(sys.argv[1:])
    except SystemExit as stop:
        code = stop.code if isinstance(stop.code, int) else 1
    except Exception as error:
        # (The swap's renames, the stamp.) The reason last, for the client.
        import traceback
        traceback.print_exc()
        print(f"error: {error}", file=sys.stderr)
        code = 1
    if "--watch-stdin" in sys.argv:
        # The watching thread still waits on stdin: leave without the
        # interpreter's shutdown, which would wait for it.
        sys.stdout.flush()
        sys.stderr.flush()
        os._exit(code)
    sys.exit(code)
