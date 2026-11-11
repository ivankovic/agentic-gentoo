"""What the tests share: the tools' directories on the import path, and fix-shadow as a module."""
import importlib.machinery
import importlib.util
import sys
from pathlib import Path

sys.dont_write_bytecode = True
ROOT = Path(__file__).resolve().parents[2]
DRILLS = ROOT / "tools" / "drills"
if str(DRILLS) not in sys.path:
    sys.path.insert(0, str(DRILLS))


def fix_shadow():
    """tools/fix/fix-shadow, a script without .py, loaded as a module."""
    path = str(ROOT / "tools" / "fix" / "fix-shadow")
    loader = importlib.machinery.SourceFileLoader("fix_shadow", path)
    module = importlib.util.module_from_spec(importlib.util.spec_from_loader("fix_shadow", loader))
    loader.exec_module(module)
    return module
