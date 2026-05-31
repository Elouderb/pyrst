# Student score management

class Student:
    name: str
    math: float
    english: float
    science: float

    def __init__(self, name: str, math: float, english: float, science: float) -> None:
        self.name = name
        self.math = math
        self.english = english
        self.science = science

def main() -> None:
    students = [
        Student("Alice", 95.0, 88.0, 92.0),
        Student("Bob", 78.0, 82.0, 80.0),
        Student("Charlie", 88.0, 91.0, 87.0),
        Student("Diana", 92.0, 95.0, 90.0),
    ]

    print("=== Student Scores ===")
    print(len(students))

    # Math scores
    total_math = 0.0
    max_math = students[0].math
    min_math = students[0].math

    for student in students:
        total_math = total_math + student.math
        if student.math > max_math:
            max_math = student.math
        if student.math < min_math:
            min_math = student.math

    print(total_math)
    print(max_math)
    print(min_math)

    avg_math = total_math / 4.0
    print(avg_math)

    # English scores
    total_english = 0.0
    for student in students:
        total_english = total_english + student.english

    avg_english = total_english / 4.0
    print(total_english)
    print(avg_english)

    # Science scores
    total_science = 0.0
    for student in students:
        total_science = total_science + student.science

    avg_science = total_science / 4.0
    print(total_science)
    print(avg_science)

    # High achievers
    high_count = 0
    for student in students:
        if student.math >= 90.0:
            high_count = high_count + 1

    print(high_count)

    # Print all students
    print("=== Student Details ===")
    for student in students:
        print(student.name)
        print(student.math)
        print(student.english)
        print(student.science)

    # Averages per student
    print("=== Averages ===")
    for student in students:
        student_avg = (student.math + student.english + student.science) / 3.0
        print(student_avg)
