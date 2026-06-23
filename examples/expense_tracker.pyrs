# Personal expense tracking

class Expense:
    category: str
    amount: float
    month: float

    def __init__(self, category: str, amount: float, month: float) -> None:
        self.category = category
        self.amount = amount
        self.month = month

def main() -> None:
    expenses = [
        Expense("Rent", 1200.0, 1.0),
        Expense("Food", 400.0, 1.0),
        Expense("Transport", 150.0, 1.0),
        Expense("Utilities", 200.0, 1.0),
        Expense("Entertainment", 100.0, 1.0),
        Expense("Rent", 1200.0, 2.0),
        Expense("Food", 450.0, 2.0),
        Expense("Transport", 160.0, 2.0),
    ]

    print("=== Expense Tracker ===")
    print(len(expenses))

    # Total expenses
    total_expenses = 0.0
    for exp in expenses:
        total_expenses = total_expenses + exp.amount

    print(total_expenses)

    # Month 1 analysis
    month1_total = 0.0
    for exp in expenses:
        if exp.month == 1.0:
            month1_total = month1_total + exp.amount

    print(month1_total)

    # Month 2 analysis
    month2_total = 0.0
    for exp in expenses:
        if exp.month == 2.0:
            month2_total = month2_total + exp.amount

    print(month2_total)

    # Highest expense
    max_expense = expenses[0].amount
    for exp in expenses:
        if exp.amount > max_expense:
            max_expense = exp.amount

    print(max_expense)

    # Average expense
    avg_expense = total_expenses / 8.0
    print(avg_expense)

    # Rent analysis
    rent_total = 0.0
    for exp in expenses:
        if exp.category == "Rent":
            rent_total = rent_total + exp.amount

    print(rent_total)

    # Food analysis
    food_total = 0.0
    for exp in expenses:
        if exp.category == "Food":
            food_total = food_total + exp.amount

    print(food_total)

    # Print all expenses
    print("=== All Expenses ===")
    for exp in expenses:
        print(exp.category)
        print(exp.amount)
        print(exp.month)

    # Category percentages
    print("=== Percentages ===")
    rent_pct = (rent_total / total_expenses) * 100.0
    food_pct = (food_total / total_expenses) * 100.0
    print(rent_pct)
    print(food_pct)

    # Large expenses
    large_count = 0
    for exp in expenses:
        if exp.amount > 300.0:
            large_count = large_count + 1

    print(large_count)

    # Monthly difference
    diff = month2_total - month1_total
    print(diff)
