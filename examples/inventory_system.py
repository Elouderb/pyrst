# Inventory system with stock management and calculations

class Item:
    code: str
    name: str
    quantity: int
    price: float

    def __init__(self, code: str, name: str, quantity: int, price: float) -> None:
        self.code = code
        self.name = name
        self.quantity = quantity
        self.price = price

    def get_value(self) -> float:
        return self.quantity * self.price

    def is_low_stock(self) -> bool:
        return self.quantity < 10

def main() -> None:
    # Create inventory
    inventory = [
        Item("A001", "Hammer", 25, 12.50),
        Item("A002", "Nails", 100, 0.50),
        Item("A003", "Wrench", 8, 15.00),
        Item("A004", "Screwdriver", 40, 5.00),
        Item("A005", "Drill", 5, 49.99),
        Item("A006", "Bits", 50, 8.00),
        Item("A007", "Saw", 12, 25.00),
        Item("A008", "Sandpaper", 30, 2.00),
    ]

    print("=== Inventory System ===")
    print(len(inventory))

    # Quantity analysis
    quantities = [i.quantity for i in inventory]
    total_quantity = sum(quantities)
    max_quantity = max(quantities)
    min_quantity = min(quantities)
    avg_quantity = total_quantity / len(quantities)

    print(total_quantity)
    print(max_quantity)
    print(min_quantity)
    print(avg_quantity)

    # Price analysis
    prices = [i.price for i in inventory]
    total_price = sum(prices)
    max_price = max(prices)
    min_price = min(prices)
    avg_price = total_price / len(prices)

    print(total_price)
    print(max_price)
    print(min_price)
    print(avg_price)

    # Inventory value
    values = [i.get_value() for i in inventory]
    total_value = sum(values)
    max_value = max(values)
    min_value = min(values)

    print(total_value)
    print(max_value)
    print(min_value)

    # Stock levels
    low_stock = [i for i in inventory if i.is_low_stock()]
    adequate_stock = [i for i in inventory if not i.is_low_stock()]

    print(len(low_stock))
    print(len(adequate_stock))

    # Price categories
    expensive = [i for i in inventory if i.price > 10.0]
    cheap = [i for i in inventory if i.price <= 10.0]

    print(len(expensive))
    print(len(cheap))

    # Print all items
    print("=== All Items ===")
    for item in inventory:
        print(item.code)
        print(item.name)
        print(item.quantity)
        print(item.price)
        value = item.get_value()
        print(value)

    # Sorted by price
    sorted_items = sorted(inventory, key=lambda i: i.price)
    print("=== Sorted by Price ===")
    for item in sorted_items:
        print(item.name)

    # Sorted by value
    sorted_by_value = sorted(inventory, key=lambda i: i.get_value())
    print("=== Sorted by Value ===")
    for item in sorted_by_value:
        print(item.name)

    # Summary
    total_items = len(inventory)
    total_units = sum(quantities)
    total_inv_value = sum(values)

    print(total_items)
    print(total_units)
    print(total_inv_value)
