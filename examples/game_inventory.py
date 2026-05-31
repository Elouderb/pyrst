class Item:
    def __init__(self, name: str, rarity: str, value: int) -> None:
        self.name = name
        self.rarity = rarity
        self.value = value

class Inventory:
    def __init__(self) -> None:
        self.items = []

    def add_item(self, item: Item) -> None:
        self.items.append(item)

    def get_total_value(self) -> int:
        return sum([item.value for item in self.items])

    def get_item_count(self) -> int:
        return len(self.items)

def main() -> None:
    # Create inventory
    inventory: Inventory = Inventory()

    # Add items
    inventory.add_item(Item("Sword", "rare", 500))
    inventory.add_item(Item("Shield", "common", 200))
    inventory.add_item(Item("Potion", "common", 50))
    inventory.add_item(Item("Ring", "epic", 1000))
    inventory.add_item(Item("Scroll", "uncommon", 150))
    inventory.add_item(Item("Amulet", "epic", 800))

    # Basic inventory info
    print("=== Inventory Summary ===")
    print(inventory.get_item_count())
    print(inventory.get_total_value())

    # Filter by rarity
    common_items = [i for i in inventory.items if i.rarity == "common"]
    uncommon_items = [i for i in inventory.items if i.rarity == "uncommon"]
    rare_items = [i for i in inventory.items if i.rarity == "rare"]
    epic_items = [i for i in inventory.items if i.rarity == "epic"]

    print(len(common_items))
    print(len(uncommon_items))
    print(len(rare_items))
    print(len(epic_items))

    # Rarity value analysis
    common_value = sum([i.value for i in common_items])
    epic_value = sum([i.value for i in epic_items])

    print(common_value)
    print(epic_value)

    # Most valuable items
    values = [i.value for i in inventory.items]
    max_value = max(values)
    min_value = min(values)

    print(max_value)
    print(min_value)

    # High value items
    valuable = [i for i in inventory.items if i.value > 200]
    print(len(valuable))

    # Item names
    all_names = [i.name for i in inventory.items]
    sorted_names = sorted(all_names)

    print("=== Sorted Items ===")
    for name in sorted_names:
        print(name)

    # Rarity distribution
    rarities = set()
    for item in inventory.items:
        rarity_set = {item.rarity}
        rarities = rarities | rarity_set

    # Value per item analysis
    print("=== Item Details ===")
    for item in inventory.items:
        print(item.name)
        print(item.rarity)
        print(item.value)

    # Inventory capacity check
    capacity = 20
    is_full = inventory.get_item_count() >= capacity
    space_left = capacity - inventory.get_item_count()

    print(is_full)
    print(space_left)

    # Weighted value by rarity
    epic_weighted = len(epic_items) * 10
    rare_weighted = len(rare_items) * 5
    other_weighted = (len(common_items) + len(uncommon_items)) * 1

    print(epic_weighted)
    print(rare_weighted)
    print(other_weighted)

    # Item eligibility
    sellable = [i for i in inventory.items if i.rarity != "epic"]
    print(len(sellable))

    # Value range analysis
    mid_value = (max_value + min_value) / 2
    above_mid = [i for i in inventory.items if i.value > mid_value]

    print(mid_value)
    print(len(above_mid))
