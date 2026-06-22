# EPIC-4 V1-bc: a ternary in a consuming position clones the ARM places, not the
# whole if-temp. `c = a if cond else b` must leave both a and b usable.
def main() -> None:
    a = [1, 2, 3]
    b = [4, 5, 6, 7]
    cond = True
    c = a if cond else b
    # mutate c; both a and b must remain independent copies
    c.append(0)
    print(len(a))
    print(len(b))
    print(len(c))
    print(c[0])

    cond2 = False
    d = a if cond2 else b
    d.append(0)
    print(len(a))
    print(len(b))
    print(len(d))

main()
