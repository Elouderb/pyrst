class Product:
    def __init__(self, name: str, price: float, stock: int) -> None:
        self.name = name
        self.price = price
        self.stock = stock

class CartItem:
    def __init__(self, product: Product, quantity: int) -> None:
        self.product = product
        self.quantity = quantity

def main() -> None:
    # Create product catalog
    products = [
        Product("Laptop", 999.99, 5),
        Product("Mouse", 29.99, 50),
        Product("Keyboard", 79.99, 20),
        Product("Monitor", 299.99, 10),
    ]

    # Build shopping cart
    cart = [
        CartItem(products[0], 1),
        CartItem(products[1], 2),
        CartItem(products[2], 1),
    ]

    # Calculate cart totals
    total_price = 0.0
    total_items = 0
    for item in cart:
        line_total = item.product.price * item.quantity
        total_price = total_price + line_total
        total_items = total_items + item.quantity

    print("=== Shopping Cart ===")
    for item in cart:
        item_name = item.product.name
        qty = item.quantity
        price = item.product.price
        line_cost = price * qty
        print(item_name)
        print(qty)
        print(line_cost)

    # Apply discount for large orders
    discount_rate = 0.0
    if total_items > 5:
        discount_rate = 0.1
    elif total_items > 3:
        discount_rate = 0.05

    discount_amount = total_price * discount_rate
    final_price = total_price - discount_amount

    print("=== Cart Summary ===")
    print(total_items)
    print(total_price)
    print(discount_rate)
    print(final_price)

    # Verify stock availability
    all_available = True
    for item in cart:
        if item.quantity > item.product.stock:
            all_available = False

    print(all_available)

    # Find most expensive item
    prices = [item.product.price for item in cart]
    max_price = max(prices)
    print(max_price)

    # Product names in uppercase
    names = [item.product.name.upper() for item in cart]
    for name in names:
        print(name)
