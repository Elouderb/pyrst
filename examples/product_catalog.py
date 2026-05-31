# Product catalog management with filtering and aggregation

class Product:
    name: str
    price: float
    quantity: int
    category: str

    def __init__(self, name: str, price: float, quantity: int, category: str) -> None:
        self.name = name
        self.price = price
        self.quantity = quantity
        self.category = category

    def get_total_value(self) -> float:
        return self.price * self.quantity

    def is_expensive(self) -> bool:
        return self.price > 50.0

def main() -> None:
    # Create catalog
    products = [
        Product("Laptop", 999.99, 5, "Electronics"),
        Product("Mouse", 29.99, 50, "Electronics"),
        Product("Desk", 199.99, 10, "Furniture"),
        Product("Chair", 149.99, 15, "Furniture"),
        Product("Monitor", 299.99, 8, "Electronics"),
        Product("Keyboard", 79.99, 20, "Electronics"),
        Product("Lamp", 39.99, 12, "Furniture"),
        Product("Pen", 2.99, 100, "Supplies"),
    ]

    print("=== Product Catalog ===")
    print(len(products))

    # Price analysis
    prices = [p.price for p in products]
    total_price = sum(prices)
    max_price = max(prices)
    min_price = min(prices)
    avg_price = total_price / len(products)

    print(total_price)
    print(max_price)
    print(min_price)
    print(avg_price)

    # Quantity analysis
    quantities = [p.quantity for p in products]
    total_quantity = sum(quantities)
    max_quantity = max(quantities)
    min_quantity = min(quantities)

    print(total_quantity)
    print(max_quantity)
    print(min_quantity)

    # Value analysis
    values = [p.get_total_value() for p in products]
    total_value = sum(values)
    max_value = max(values)
    min_value = min(values)

    print(total_value)
    print(max_value)
    print(min_value)

    # Category analysis
    electronics = [p for p in products if p.category == "Electronics"]
    furniture = [p for p in products if p.category == "Furniture"]
    supplies = [p for p in products if p.category == "Supplies"]

    print(len(electronics))
    print(len(furniture))
    print(len(supplies))

    # Price category
    expensive = [p for p in products if p.is_expensive()]
    affordable = [p for p in products if not p.is_expensive()]

    print(len(expensive))
    print(len(affordable))

    # Stock levels
    in_stock = [p for p in products if p.quantity > 0]
    low_stock = [p for p in products if p.quantity < 10]

    print(len(in_stock))
    print(len(low_stock))

    # Print sorted products
    print("=== Products by Price ===")
    sorted_products = sorted(products, key=lambda p: p.price)
    for p in sorted_products:
        print(p.name)
        print(p.price)
