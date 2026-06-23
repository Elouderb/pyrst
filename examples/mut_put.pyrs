# EPIC-4 V2-c: Mut[dict] by-reference param. `put` writes into the CALLER's
# dict in place; deterministic key lookups afterward prove persistence (dict
# iteration order is unspecified, so we look up specific keys rather than
# printing the whole map).
def put(env: Mut[dict[str, int]], k: str, v: int) -> None:
    env[k] = v

def main() -> None:
    table: dict[str, int] = {}
    put(table, "a", 1)
    put(table, "b", 2)
    put(table, "a", 9)
    print(len(table))
    print(table["a"])
    print(table["b"])
