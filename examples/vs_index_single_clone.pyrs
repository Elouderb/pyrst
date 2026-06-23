# EPIC-4 V1-bc: list-index reads already self-clone (the index lowering ends in
# `__list[__idx].clone()`). emit_consuming must pass Index THROUGH unchanged, so
# consuming a list index in a constructor/append/assign produces exactly ONE
# extracted-element clone — no double-clone. This program exercises the index
# value at several consuming sites; correct single-clone behavior is observable
# as: the extracted element is an independent copy of the original.
def main() -> None:
    grid = [[1, 2], [3, 4]]
    # assign from index (consuming) — single clone of the element
    row = grid[0]
    row.append(9)
    print(len(row))
    print(len(grid[0]))
    # append an index element to another list (consuming)
    acc = []
    acc.append(grid[1])
    grid[1].append(8)
    print(len(acc[0]))
    print(len(grid[1]))

main()
