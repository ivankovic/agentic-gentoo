"""Shared by the tests here: the repository's root, and a script loaded as a module by its path
(most have no .py in their name). Nothing here touches the system: the scripts are the
repository's copies, not the installed ones."""
import importlib.machinery
import importlib.util
import pathlib
import sys

sys.dont_write_bytecode = True  # no __pycache__ beside the scripts that get deployed

ROOT = pathlib.Path(__file__).resolve().parents[2]
DECK = ROOT / "hosts/deck"


def script(path, name=None):
    path = pathlib.Path(path)
    loader = importlib.machinery.SourceFileLoader(name or path.name.replace("-", "_").replace(".", "_"), str(path))
    module = importlib.util.module_from_spec(importlib.util.spec_from_loader(loader.name, loader))
    loader.exec_module(module)
    return module
