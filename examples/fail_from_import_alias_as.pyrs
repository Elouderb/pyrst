# EPIC-6 B (card 0aac2461): `from X import Y as Z` — the `as` alias is not
# yet supported. Before this card the alias was silently discarded, so `Z` was
# unusable while `Y` was visible under its original name (silent wrong-
# acceptance). The honest behavior is to reject this form at parse time.
# This file must be rejected by BOTH `pyrst check` and `pyrst build`.
from math import floor as f

def main() -> None:
    print(f(3.7))
