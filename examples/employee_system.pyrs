# Employee management system with classes and data processing

class Employee:
    name: str
    salary: float
    years: int
    department: str

    def __init__(self, name: str, salary: float, years: int, department: str) -> None:
        self.name = name
        self.salary = salary
        self.years = years
        self.department = department

    def get_bonus(self) -> float:
        base_bonus = self.salary * 0.1
        tenure_bonus = self.years * 100.0
        return base_bonus + tenure_bonus

    def is_senior(self) -> bool:
        return self.years >= 5

def main() -> None:
    # Create employees
    employees = [
        Employee("Alice", 80000.0, 7, "Engineering"),
        Employee("Bob", 75000.0, 3, "Sales"),
        Employee("Charlie", 85000.0, 10, "Engineering"),
        Employee("Diana", 70000.0, 2, "HR"),
        Employee("Eve", 90000.0, 8, "Management"),
    ]

    print("=== Employee System ===")
    print(len(employees))

    # Calculate bonuses
    bonuses = [emp.get_bonus() for emp in employees]
    total_bonuses = sum(bonuses)
    avg_bonus = total_bonuses / len(employees)

    print(total_bonuses)
    print(avg_bonus)

    # Department analysis
    engineering = [e for e in employees if e.department == "Engineering"]
    sales = [e for e in employees if e.department == "Sales"]
    other = [e for e in employees if e.department != "Engineering" and e.department != "Sales"]

    print(len(engineering))
    print(len(sales))
    print(len(other))

    # Salary analysis
    salaries = [e.salary for e in employees]
    total_salary = sum(salaries)
    max_salary = max(salaries)
    min_salary = min(salaries)
    avg_salary = total_salary / len(employees)

    print(total_salary)
    print(max_salary)
    print(min_salary)
    print(avg_salary)

    # Seniority analysis
    senior = [e for e in employees if e.is_senior()]
    junior = [e for e in employees if not e.is_senior()]

    print(len(senior))
    print(len(junior))

    # Print employee details
    print("=== Employee Details ===")
    for emp in employees:
        print(emp.name)
        print(emp.salary)
        print(emp.years)
        bonus = emp.get_bonus()
        print(bonus)

    # Sort by salary
    sorted_by_salary = sorted(employees, key=lambda e: e.salary)
    print("=== Sorted by Salary ===")
    for emp in sorted_by_salary:
        print(emp.name)

    # Calculate total compensation
    total_comp = sum([e.salary + e.get_bonus() for e in employees])
    print(total_comp)
