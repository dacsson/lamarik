#!/usr/bin/env python3
"""
ATTENTION: This script is entirely vibe-coded

regression.py – compile .lama files, run the target interpreter (lamarik)
and optionally compare with the reference implementation (lamac).

Features
--------
* Works on a folder (or a list of files) that contains *.lama* and optional
  *.input* files.
* Compiles each *.lama* to byte‑code (*.bc*) using the same command the
  original Makefile’s “hex” target uses.
* Runs the target interpreter `./target/release/lama-rs -l <bc>` feeding the
  matching *.input* file to stdin.
* Detects **runtime failures**:
    - non‑zero exit code, **or**
    - the literal string `*** FAILURE:` in stdout/stderr.
* Optionally runs the reference implementation (`lamac -i` or `lamac -s`) and
  compares the outputs.
* Prints a concise per‑file line, a final summary and writes all failing
  test details to `failures.log`.
* Never aborts because a single test fails – useful for CI pipelines.

Usage
-----
    python3 regression.py [options] <path>

    <path> can be a directory that contains *.lama* files (and optional
    *.input* files) or one or more explicit *.lama* files.  If omitted the
    current directory is used.

Options
-------
  -r, --reference {i,s}
        Run reference implementation (lamac) in mode i (interpreter) or s
        (stack‑machine) and compare its output with the target interpreter.
"""

# ----------------------------------------------------------------------
# Imports
# ----------------------------------------------------------------------
import argparse
import os
import shutil
import subprocess
import sys
import time
from pathlib import Path
from typing import List, Tuple
import re

# ----------------------------------------------------------------------
# Configuration – mirrors the original Makefile
# ----------------------------------------------------------------------
LAMA_PATH   = Path(os.getenv("LAMA_PATH", Path.cwd().parent / "Lama"))
LAMAC       = Path(os.getenv("LAMAC", LAMA_PATH / "src" / "lamac"))
RUNTIME_DIR = Path(os.getenv("RUNTIME_DIR", LAMA_PATH / "runtime"))
STD_LIB_DIR = Path(os.getenv("STD_LIB_DIR", LAMA_PATH / "stdlib" / "x64"))
LAMARIK     = Path(os.getenv("LAMARIK", "./target/release/lama-rs"))
DUMP_DIR    = Path("./dump")               # where *.bc* files are placed
FAIL_LOG    = Path("failures.log")

# ----------------------------------------------------------------------
# Helper utilities
# ----------------------------------------------------------------------
def run_cmd(cmd: List[str], stdin: bytes | str | None = None) -> Tuple[int, str, str]:
    """
    Execute *cmd*.
    *stdin* may be ``bytes`` (binary) or ``str`` (text).  Returns
    (returncode, stdout, stderr) – both stdout and stderr are always decoded
    to ``str``.
    """
    use_binary = isinstance(stdin, (bytes, bytearray))

    try:
        proc = subprocess.run(
            cmd,
            input=stdin,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=not use_binary,          # text=False when stdin is bytes
            check=False,
        )
    except FileNotFoundError:
        sys.exit(f"Executable not found: {cmd[0]}")
    except Exception as exc:
        sys.exit(f"Error running {' '.join(cmd)}: {exc}")

    # Decode only when we got raw bytes.
    stdout = proc.stdout if isinstance(proc.stdout, str) else proc.stdout.decode(errors="replace")
    stderr = proc.stderr if isinstance(proc.stderr, str) else proc.stderr.decode(errors="replace")
    return proc.returncode, stdout, stderr


def compile_lama(src: Path) -> Path:
    """Compile a .lama file to .bc and move the result into ./dump/."""
    if not src.is_file():
        raise FileNotFoundError(src)

    DUMP_DIR.mkdir(parents=True, exist_ok=True)

    cmd = [
        str(LAMAC),
        "-64",
        str(src),
        "-I",
        str(STD_LIB_DIR),
        "-runtime",
        str(RUNTIME_DIR),
        "-b",
    ]
    rc, out, err = run_cmd(cmd)
    if rc != 0:
        raise RuntimeError(f"Compilation failed for {src}\n{err}")

    generated = Path.cwd() / f"{src.stem}.bc"

    if generated is None:
        # Give a clear error that also tells the user where we looked.
        searched = Path.cwd() / f"{src.stem}.bc"
        raise RuntimeError(
            f"Expected bytecode file not produced. Searched: {searched}\n"
            f"Compiler stdout:\n{out}\n"
            f"Compiler stderr:\n{err}"
        )

    dest = DUMP_DIR / generated.name
    shutil.move(str(generated), str(dest))
    return dest


def run_lamarik(bc: Path, inp: Path | None) -> Tuple[int, float, str]:
    """Run the target interpreter on *bc*; return (exitcode, seconds, output)."""
    print(f"Running {bc}")
    stdin = inp.read_bytes() if inp and inp.is_file() else None
    cmd = [str(LAMARIK), "-l", str(bc)]
    start = time.perf_counter()
    rc, out, err = run_cmd(cmd, stdin=stdin)
    elapsed = time.perf_counter() - start
    full_out = (out + "\n" + err).strip()
    return rc, elapsed, full_out


def run_reference(src: Path, inp: Path | None, mode: str) -> Tuple[int, float, str]:
    """Run the reference interpreter (lamac) in mode '-i' or '-s'."""
    if mode not in ("i", "s"):
        raise ValueError("mode must be 'i' or 's'")
    cmd = [str(LAMAC), f"-{mode}", str(src)]
    stdin = inp.read_bytes() if inp and inp.is_file() else None
    start = time.perf_counter()
    rc, out, err = run_cmd(cmd, stdin=stdin)
    elapsed = time.perf_counter() - start
    full_out = (out + "\n" + err).strip()
    return rc, elapsed, full_out


def input_file_for(src: Path) -> Path | None:
    """Return the matching *.input* file (or None)."""
    cand = src.with_suffix(".input")
    return cand if cand.is_file() else None

def answer_file_for(src: Path) -> Path | None:
    tfile = src.with_suffix(".t")
    return tfile if tfile.is_file() else None


def extract_numbers_from_file(file_path: Path) -> list[int]:
    """
    Extract numbers from a file containing command output like:
      $ ../src/Driver.exe ... < test084.input
       > 55
      310
      310

    Returns: [55, 310, 310]
    Only extracts numbers that:
    - Appear after '>' symbol (like "> 55")
    - Appear at the beginning of a line (like "310")
    """
    numbers = []

    with open(file_path, 'r') as f:
        for line in f:
            line = line.rstrip()  # Remove trailing whitespace but keep leading

            # Skip empty lines
            if not line:
                continue

            # Case 1: Line starts with '>' followed by a number
            if '>' in line:
                # Extract number after '>'
                match = re.search(r'>\s*(-?\d+)', line)
                if match:
                    numbers.append(int(match.group(1)))

            # Case 2: Line starts with a number (after trimming leading spaces)
            elif line.strip() and (line.strip()[0].isdigit() or line.strip()[0] == '-'):
                # Extract the number at the beginning of the line
                match = re.match(r'^\s*(-?\d+)', line)
                if match:
                    numbers.append(int(match.group(1)))

    return numbers

def extract_numbers_from_output(output_text: str) -> list[int]:
    """
    Extract numbers from output text like:
    > 55
    310
    310

    Returns: [55, 310, 310]
    """
    numbers = []

    # Split the text into lines
    for line in output_text.strip().split('\n'):
        line = line.strip()

        # Skip empty lines
        if not line:
            continue

        # Find all integers in the line
        found_numbers = re.findall(r'-?\d+', line)

        # Convert to integers and add to list
        for num_str in found_numbers:
            try:
                numbers.append(int(num_str))
            except ValueError:
                pass

    return numbers

def is_runtime_failure(rc: int, output: str) -> bool:
    """
    Decide whether the interpreter run has *failed*.
    Failure is defined as:
        * non‑zero exit code, OR
        * the literal string "*** FAILURE:" appearing in stdout/stderr.
    """
    return rc != 0 or "*** FAILURE:" in output


# ----------------------------------------------------------------------
# Main driver
# ----------------------------------------------------------------------
def main() -> None:
    parser = argparse.ArgumentParser(
        description=__doc__,
        formatter_class=argparse.RawDescriptionHelpFormatter,
    )
    parser.add_argument(
        "paths",
        nargs="*",
        type=Path,
        help="file(s) or a directory containing *.lama files",
    )
    parser.add_argument(
        "-r",
        "--reference",
        choices=["i", "s"],
        metavar="MODE",
        help="run reference implementation (i = interpreter, s = stack‑machine) and compare",
    )
    args = parser.parse_args()

    # --------------------------------------------------------------
    # Build list of *.lama* files
    # --------------------------------------------------------------
    if not args.paths:
        search_root = Path.cwd()
        lama_files = list(search_root.rglob("*.lama"))
    else:
        lama_files = []
        for pth in args.paths:
            if pth.is_dir():
                lama_files.extend(pth.rglob("*.lama"))
            elif pth.suffix == ".lama":
                lama_files.append(pth)
            else:
                sys.exit(f"Unsupported argument: {pth} (must be .lama or a directory)")

    lama_files = [p.resolve() for p in lama_files]
    if not lama_files:
        sys.exit("No .lama files found to test.")

    # --------------------------------------------------------------
    # Statistics containers
    # --------------------------------------------------------------
    total = len(lama_files)
    passed = 0
    failed = 0
    target_time = 0.0
    ref_time = 0.0

    if FAIL_LOG.is_file():
        FAIL_LOG.unlink()                     # start fresh

    print(f"Testing {total} file(s)...\n")

    # --------------------------------------------------------------
    # Process each file – never abort on error
    # --------------------------------------------------------------
    for src in lama_files:
        try:
            # 1️⃣ Compile
            bc = compile_lama(src)

            # 2️⃣ Run target interpreter
            inp = input_file_for(src)
            t_rc, t_sec, t_out = run_lamarik(bc, inp)
            target_time += t_sec

            # parse answers file
            tfile = answer_file_for(src)
            if tfile is None:
                sys.exit(f"Answers file {src}.t not foun")

            answers = extract_numbers_from_file(tfile)
            lamarik_answers = extract_numbers_from_output(t_out)

            # 3️⃣ Decide PASS/FAIL for the target run
            ok = not is_runtime_failure(t_rc, t_out)

            # 4️⃣ (optional) reference run & comparison
            if args.reference:
                r_rc, r_sec, r_out = run_reference(src, inp, args.reference)
                ref_time += r_sec
                # If the reference succeeded, also require equality of outputs
                if ok and r_rc == 0 and t_out == r_out:
                    ok = True
                else:
                    ok = False

            # 5️⃣ Record outcome
            if ok and (lamarik_answers == answers):
                passed += 1
                status = "PASS"
            else:
                failed += 1
                status = "FAIL"
                # Log detailed information for later inspection
                with FAIL_LOG.open("a") as log:
                    log.write(f"{src.name}:\n")
                    log.write("---- Target output ----\n")
                    log.write(t_out + "\n")
                    if (lamarik_answers != answers) and ok:
                        log.write("\n---- Golden answers ----\n")
                        log.write(str(answers))
                        log.write("\n---- Lamarik answres ----\n")
                        log.write(str(lamarik_answers))
                    if args.reference:
                        log.write("\n---- Reference output ----\n")
                        log.write(r_out + "\n")
                    log.write("\n")

            # One‑line per‑test summary
            line = f"{src.name:30} [{status}]  target:{t_sec:0.3f}s"
            if args.reference:
                line += f"  ref({args.reference}):{r_sec:0.3f}s"
            print(line)

        except Exception as exc:
            # Anything unexpected counts as a failure but does NOT stop the run
            failed += 1
            print(f"{src.name:30} [ERROR] {exc}")
            with FAIL_LOG.open("a") as log:
                log.write(f"{src.name}: EXCEPTION\n{exc}\n\n")

    # --------------------------------------------------------------
    # Global summary
    # --------------------------------------------------------------
    print("\n" + "=" * 60)
    print("Summary")
    print("-" * 60)
    print(f"Total   : {total}")
    print(f"Passed  : {passed}")
    print(f"Failed  : {failed}")
    print(f"Target  : {target_time:0.3f}s")
    if args.reference:
        print(f"Reference({args.reference}) : {ref_time:0.3f}s")
    if failed and FAIL_LOG.is_file():
        print(f"\nDetails of failures are stored in {FAIL_LOG}")

    if passed == 75:
        print("\nAll tests passed!")
        sys.exit(0)

    # Exit with non‑zero status when there are failures (useful for CI)
    sys.exit(1 if failed else 0)


if __name__ == "__main__":
    main()
