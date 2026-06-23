# EPIC-4 V1-bc: appending a non-Copy variable to a list must clone the variable,
# so the variable remains independently usable after the append (the list owns a
# copy, not the original binding).
def main() -> None:
    bucket = []
    chunk = ["a", "b"]
    bucket.append(chunk)
    # mutate the original chunk; the appended copy must be independent
    chunk.append("c")
    # reuse chunk after it was consumed by append
    print(len(chunk))
    print(len(bucket))
    print(len(bucket[0]))

main()
