"""Command-line interface for building Motyga package directories."""

from __future__ import annotations

import argparse
import tempfile
from pathlib import Path

from .archive import write_archive
from .cargo import build_source_binaries
from .layout import build_package_dir
from .layout import prepare_package_dir
from .layout import validate_package_dir
from .ripgrep import resolve_rg_bin
from .targets import PACKAGE_VARIANTS
from .targets import TARGET_SPECS
from .targets import PackageInputs
from .targets import default_target
from .targets import resolve_input_path
from .zsh import resolve_zsh_bin
from .version import read_workspace_version


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Build a canonical Motyga package directory and optional archive.",
        formatter_class=argparse.ArgumentDefaultsHelpFormatter,
    )
    parser.add_argument(
        "--target",
        default=argparse.SUPPRESS,
        choices=sorted(TARGET_SPECS),
        help=(
            "Rust target triple for the package. Defaults to the release target "
            "for this host platform."
        ),
    )
    parser.add_argument(
        "--variant",
        choices=sorted(PACKAGE_VARIANTS),
        default="motyga",
        help="Package variant to build.",
    )
    parser.add_argument(
        "--package-dir",
        type=Path,
        default=argparse.SUPPRESS,
        help=(
            "Output directory to create as the package root. Defaults to a new temporary directory."
        ),
    )
    parser.add_argument(
        "--archive-output",
        type=Path,
        action="append",
        default=[],
        help=(
            "Optional archive output path. May be repeated. Supported suffixes: "
            ".tar.gz, .tgz, .tar.zst, .zip."
        ),
    )
    parser.add_argument(
        "--force",
        action="store_true",
        help="Replace an existing package directory or archive output.",
    )
    parser.add_argument(
        "--cargo",
        default="cargo",
        help="Cargo executable to use for source-built package artifacts.",
    )
    parser.add_argument(
        "--cargo-profile",
        default="dev-small",
        help=(
            "Cargo profile for source-built package artifacts. Use release for release packages."
        ),
    )
    parser.add_argument(
        "--entrypoint-bin",
        type=Path,
        help=(
            "Optional prebuilt entrypoint executable for the selected package "
            "variant. If omitted, the entrypoint is built with Cargo."
        ),
    )
    parser.add_argument(
        "--bwrap-bin",
        type=Path,
        help=(
            "Optional prebuilt Linux bwrap executable. If omitted for Linux "
            "targets, bwrap is built with Cargo."
        ),
    )
    parser.add_argument(
        "--zsh-manifest",
        type=Path,
        help=(
            "Optional DotSlash manifest for the patched zsh fork instead of "
            "scripts/motyga_package/codex-zsh."
        ),
    )
    parser.add_argument(
        "--motyga-command-runner-bin",
        type=Path,
        help=(
            "Optional prebuilt Windows motyga-command-runner.exe executable. "
            "If omitted for Windows targets, motyga-command-runner is built "
            "with Cargo."
        ),
    )
    parser.add_argument(
        "--motyga-windows-sandbox-setup-bin",
        type=Path,
        help=(
            "Optional prebuilt Windows motyga-windows-sandbox-setup.exe "
            "executable. If omitted for Windows targets, "
            "motyga-windows-sandbox-setup is built with Cargo."
        ),
    )
    parser.add_argument(
        "--rg-bin",
        type=Path,
        help=(
            "Optional local ripgrep executable override instead of fetching from "
            "scripts/motyga_package/rg."
        ),
    )
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    spec = TARGET_SPECS[getattr(args, "target", None) or default_target()]
    variant = PACKAGE_VARIANTS[args.variant]
    package_dir_arg = getattr(args, "package_dir", None)
    package_dir = (
        package_dir_arg.resolve()
        if package_dir_arg is not None
        else Path(tempfile.mkdtemp(prefix="motyga-package-")).resolve()
    )

    source_outputs = build_source_binaries(
        spec,
        variant,
        cargo=args.cargo,
        profile=args.cargo_profile,
        entrypoint_bin=resolve_optional_input_path(
            args.entrypoint_bin,
            "prebuilt entrypoint executable",
            "--entrypoint-bin",
        ),
        bwrap_bin=resolve_optional_input_path(
            args.bwrap_bin,
            "prebuilt Linux bwrap executable",
            "--bwrap-bin",
        ),
        motyga_command_runner_bin=resolve_optional_input_path(
            args.motyga_command_runner_bin,
            "prebuilt Windows motyga-command-runner.exe executable",
            "--motyga-command-runner-bin",
        ),
        motyga_windows_sandbox_setup_bin=resolve_optional_input_path(
            args.motyga_windows_sandbox_setup_bin,
            "prebuilt Windows motyga-windows-sandbox-setup.exe executable",
            "--motyga-windows-sandbox-setup-bin",
        ),
    )
    version = read_workspace_version()
    inputs = PackageInputs(
        entrypoint_bin=source_outputs.entrypoint_bin,
        rg_bin=resolve_rg_bin(spec, args.rg_bin),
        zsh_bin=resolve_zsh_bin(spec, args.zsh_manifest),
        bwrap_bin=source_outputs.bwrap_bin,
        motyga_command_runner_bin=source_outputs.motyga_command_runner_bin,
        motyga_windows_sandbox_setup_bin=source_outputs.motyga_windows_sandbox_setup_bin,
    )
    prepare_package_dir(package_dir, force=args.force)
    build_package_dir(package_dir, version, variant, spec, inputs)
    validate_package_dir(
        package_dir, variant, spec, include_zsh=inputs.zsh_bin is not None
    )

    for archive_output in args.archive_output:
        archive_path = archive_output.resolve()
        write_archive(package_dir, archive_path, force=args.force)
        print(f"Built Motyga package archive at {archive_path}")

    print(f"Built Motyga package directory at {package_dir}")
    return 0


def resolve_optional_input_path(
    explicit_path: Path | None,
    description: str,
    flag_name: str,
) -> Path | None:
    if explicit_path is None:
        return None

    return resolve_input_path(explicit_path, description, flag_name)
