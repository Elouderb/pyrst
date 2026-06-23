# Financial calculator with mixed int and float operations

class Account:
    name: str
    balance: float
    interest_rate: float

    def __init__(self, name: str, balance: float, interest_rate: float) -> None:
        self.name = name
        self.balance = balance
        self.interest_rate = interest_rate

def main() -> None:
    # Create accounts
    savings = Account("Savings", 1000.0, 0.05)
    checking = Account("Checking", 500.0, 0.01)

    print("=== Financial Calculator ===")

    # Simple interest calculation (int + float)
    months = 12
    savings_interest = savings.balance * savings.interest_rate * months / 12.0
    print(savings_interest)

    checking_interest = checking.balance * checking.interest_rate * months / 12.0
    print(checking_interest)

    # Total balance (float + float)
    total_balance = savings.balance + checking.balance
    print(total_balance)

    # Compound interest (int * float)
    years = 2
    compound_factor = (1.0 + savings.interest_rate) * years
    compound_balance = savings.balance * compound_factor
    print(compound_balance)

    # Division operations (int / float)
    monthly_savings = savings.balance / 12.0
    print(monthly_savings)

    # Mixed arithmetic
    total_interest = savings_interest + checking_interest
    net_gain = total_balance - (savings.balance + checking.balance)
    print(total_interest)
    print(net_gain)

    # Percentage calculations (int / int returns float)
    deposit1 = 100
    deposit2 = 50
    ratio = deposit1 / deposit2
    print(ratio)

    # Combined operations
    total_deposits = 100 + 50
    average = total_deposits / 3.0
    print(average)

    # Tax calculation (float * float)
    tax_rate = 0.15
    tax_amount = total_interest * tax_rate
    after_tax = total_interest - tax_amount
    print(tax_amount)
    print(after_tax)

    # Multiple mixed operations
    result1 = 100 + 50.5
    result2 = result1 * 2
    result3 = result2 / 3.0
    print(result1)
    print(result2)
    print(result3)

    # Nested calculations
    value = (1000 + 500.50) * (1.0 + 0.05) / 2
    print(value)
