# EPIC-7 sub-item 2: dict.get(k) returns Optional[V] (None when absent), so the
# result narrows with `is None`. dict.get(k, default) still returns V directly.
def main() -> None:
    ages: dict[str, int] = {"alice": 30, "bob": 25}

    found = ages.get("alice")
    if found is None:
        print("missing")
    else:
        print(found)

    absent = ages.get("carol")
    if absent is None:
        print("missing")
    else:
        print(absent)

    # Two-arg form returns the value type directly (the supplied fallback).
    with_default: int = ages.get("dave", -1)
    print(with_default)
    present: int = ages.get("bob", -1)
    print(present)
