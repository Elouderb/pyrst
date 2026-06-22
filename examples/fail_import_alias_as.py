# EPIC-6 B (card 0aac2461): `import X as Y` — the `as` alias is not yet
# supported. Before this card the alias was silently discarded, leaving `Y`
# unusable while `X` was visible under its original name (silent wrong-
# acceptance). The honest behavior is to reject this form at parse time.
# This file must be rejected by BOTH `pyrst check` and `pyrst build`.
import math as m

def main() -> None:
    print(m.floor(3.7))
