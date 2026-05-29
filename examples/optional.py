def find_item(lst: list[int], target: int) -> int:
    for item in lst:
        if item == target:
            return item
    return -1

def main() -> None:
    items: list[int] = [1, 2, 3, 4, 5]
    result: int = find_item(items, 3)
    if result != -1:
        print("found")
    else:
        print("not found")
