# Fruit store - simple inventory analysis

class Fruit:
    name: str
    price: float
    stock: float

    def __init__(self, name: str, price: float, stock: float) -> None:
        self.name = name
        self.price = price
        self.stock = stock

def main() -> None:
    fruits = [
        Fruit("Apple", 1.50, 30.0),
        Fruit("Banana", 0.75, 50.0),
        Fruit("Orange", 2.00, 25.0),
        Fruit("Grape", 3.00, 15.0),
        Fruit("Mango", 2.50, 12.0),
    ]

    print("=== Fruit Store ===")
    print(len(fruits))

    # Calculate totals
    total_price = 0.0
    total_stock = 0.0
    total_value = 0.0

    for fruit in fruits:
        total_price = total_price + fruit.price
        total_stock = total_stock + fruit.stock
        total_value = total_value + (fruit.price * fruit.stock)

    print(total_price)
    print(total_stock)
    print(total_value)

    # Averages
    count = 5.0
    avg_price = total_price / count
    avg_stock = total_stock / count

    print(avg_price)
    print(avg_stock)

    # Find extremes
    max_price = fruits[0].price
    min_price = fruits[0].price
    max_stock = fruits[0].stock
    min_stock = fruits[0].stock

    for fruit in fruits:
        if fruit.price > max_price:
            max_price = fruit.price
        if fruit.price < min_price:
            min_price = fruit.price
        if fruit.stock > max_stock:
            max_stock = fruit.stock
        if fruit.stock < min_stock:
            min_stock = fruit.stock

    print(max_price)
    print(min_price)
    print(max_stock)
    print(min_stock)

    # Print all items
    print("=== Inventory Details ===")
    for fruit in fruits:
        print(fruit.name)
        print(fruit.price)
        print(fruit.stock)

    # Count stats
    count_expensive = 0
    count_cheap = 0

    for fruit in fruits:
        if fruit.price > 2.0:
            count_expensive = count_expensive + 1
        else:
            count_cheap = count_cheap + 1

    print(count_expensive)
    print(count_cheap)

    # Stock analysis
    total_low = 0
    for fruit in fruits:
        if fruit.stock < 20.0:
            total_low = total_low + 1

    total_high = len(fruits) - total_low
    print(total_low)
    print(total_high)

    # Value per fruit
    print("=== Value Analysis ===")
    fruit1_val = fruits[0].price * fruits[0].stock
    fruit2_val = fruits[1].price * fruits[1].stock
    fruit3_val = fruits[2].price * fruits[2].stock
    fruit4_val = fruits[3].price * fruits[3].stock
    fruit5_val = fruits[4].price * fruits[4].stock

    print(fruit1_val)
    print(fruit2_val)
    print(fruit3_val)
    print(fruit4_val)
    print(fruit5_val)

    # Category analysis
    expensive_value = 0.0
    cheap_value = 0.0

    for fruit in fruits:
        fruit_value = fruit.price * fruit.stock
        if fruit.price > 2.0:
            expensive_value = expensive_value + fruit_value
        else:
            cheap_value = cheap_value + fruit_value

    print(expensive_value)
    print(cheap_value)
