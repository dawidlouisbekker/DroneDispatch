#!/usr/bin/env python3
"""Generate the auth service's PKCS#8 P-256 signing key."""

from argparse import ArgumentParser
from pathlib import Path
import os
import shutil
import subprocess


REPOSITORY_ROOT = Path(__file__).resolve().parent.parent
DEFAULT_OUTPUT = REPOSITORY_ROOT / "services/auth-service/auth-signing-key.pem"


def main() -> None:
    parser = ArgumentParser(description=__doc__)
    parser.add_argument(
        "output",
        nargs="?",
        type=Path,
        default=DEFAULT_OUTPUT,
        help=f"output PEM path (default: {DEFAULT_OUTPUT})",
    )
    args = parser.parse_args()

    openssl = shutil.which("openssl")
    if openssl is None:
        raise SystemExit("openssl is required but was not found on PATH")

    args.output.parent.mkdir(parents=True, exist_ok=True)
    if args.output.exists():
        raise SystemExit(f"refusing to overwrite existing key: {args.output}")

    subprocess.run(
        [
            openssl,
            "genpkey",
            "-algorithm",
            "EC",
            "-pkeyopt",
            "ec_paramgen_curve:P-256",
            "-out",
            str(args.output),
        ],
        check=True,
    )
    os.chmod(args.output, 0o600)
    print(f"generated {args.output} (PKCS#8 P-256 private key)")


if __name__ == "__main__":
    main()