# File I/O via open() and the with-statement. The file closes (RAII) when the
# block ends. Modes: "r" (default), "w", "a". readlines() strips line endings.
def main() -> None:
    path: str = "/tmp/pyrst_fileio_example.txt"
    with open(path, "w") as f:
        f.write("alpha\n")
        f.write("beta\n")
    with open(path, "a") as f:
        f.write("gamma\n")
    with open(path) as g:
        lines: list[str] = g.readlines()
        print(len(lines))
        print(lines[0])
        print(lines[1])
        print(lines[2])
    with open(path) as g:
        text: str = g.read()
        print(len(text))
