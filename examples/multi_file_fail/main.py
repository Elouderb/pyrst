# EPIC-8 multi-file error-sourcing NEGATIVE fixture (the root module).
# Importing `broken` from lib.py pulls in a type error that originates in lib.py.
# `pyrst check`/`build` must REJECT this program, and the diagnostic must name
# lib.py (not main.py) and show lib.py's offending line + caret.
from lib import add_one, broken

def main() -> None:
    print(add_one(5))
    print(broken(10))
