def main() -> None:
    path: str = "/tmp/pyrst_corpus_data.csv"
    with open(path, "w") as f:
        f.write("alice,30,nyc\n")
        f.write("bob,25,la\n")
    with open(path) as g:
        rows: list[str] = g.readlines()
        names: list[str] = []
        ages: list[int] = []
        for row in rows:
            fields: list[str] = row.split(",")
            names.append(fields[0])
            ages.append(int(fields[1]))
        print(len(rows))
        print(names[0])
        print(ages[1])
        print(sum(ages))
