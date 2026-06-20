def get_items(count: int) -> list[int]:
    if count > 0:
        items: list[int] = [1, 2, 3]
        return items
    else:
        empty: list[int] = []
        return empty

def process_data(records: list[dict[str, list[int]]]) -> int:
    total: int = 0
    for record in records:
        values: list[int] = record["data"]
        total = total + len(values)
    return total

def main() -> None:
    # Test 1: Ternary with empty vs non-empty list branches
    num_items: int = 3
    result_list: list[int] = [10, 20] if num_items > 0 else []
    print(len(result_list))
    
    # Test 2: Nested dict with list values, immediate access
    nested: dict[str, list[int]] = {"x": [1, 2, 3], "y": [4, 5]}
    first_elem: int = nested["x"][0]
    print(first_elem)
    
    # Test 3: Chained method on converted string, then len()
    s: str = "HELLO"
    lowered: str = s.lower()
    char_count: int = len(lowered)
    print(char_count)
    
    # Test 4: Function returning list, immediately indexed
    items: list[int] = get_items(5)
    if len(items) > 0:
        first: int = items[0]
        print(first)
    
    # Test 5: Collection of dicts with nested lists
    data_records: list[dict[str, list[int]]] = [
        {"data": [7, 8, 9]},
        {"data": [10]}
    ]
    total_count: int = process_data(data_records)
    print(total_count)