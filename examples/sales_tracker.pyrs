# Sales tracking system

class Sale:
    item: str
    quantity: float
    price: float
    discount: float

    def __init__(self, item: str, quantity: float, price: float, discount: float) -> None:
        self.item = item
        self.quantity = quantity
        self.price = price
        self.discount = discount

def main() -> None:
    sales = [
        Sale("Laptop", 5.0, 999.0, 0.1),
        Sale("Monitor", 8.0, 299.0, 0.05),
        Sale("Keyboard", 20.0, 79.0, 0.0),
        Sale("Mouse", 15.0, 29.0, 0.15),
        Sale("Desk", 10.0, 199.0, 0.1),
    ]

    print("=== Sales Tracker ===")
    print(len(sales))

    # Total sales
    total_revenue = 0.0
    total_items = 0.0

    for sale in sales:
        revenue = sale.quantity * sale.price
        total_revenue = total_revenue + revenue
        total_items = total_items + sale.quantity

    print(total_revenue)
    print(total_items)

    # Discounts
    total_discount = 0.0
    for sale in sales:
        discount_amount = sale.quantity * sale.price * sale.discount
        total_discount = total_discount + discount_amount

    print(total_discount)

    final_revenue = total_revenue - total_discount
    print(final_revenue)

    # High volume
    high_volume = 0
    for sale in sales:
        if sale.quantity > 10.0:
            high_volume = high_volume + 1

    print(high_volume)

    # Premium items
    premium_total = 0.0
    for sale in sales:
        if sale.price > 200.0:
            premium_total = premium_total + sale.quantity

    print(premium_total)

    # Print sales details
    print("=== Sales Details ===")
    for sale in sales:
        print(sale.item)
        print(sale.quantity)
        print(sale.price)
        print(sale.discount)

    # Revenue per item
    print("=== Revenue Analysis ===")
    for sale in sales:
        item_revenue = sale.quantity * sale.price
        after_discount = item_revenue * (1.0 - sale.discount)
        print(item_revenue)
        print(after_discount)

    # Average statistics
    avg_quantity = total_items / 5.0
    avg_price_paid = final_revenue / total_items
    print(avg_quantity)
    print(avg_price_paid)
