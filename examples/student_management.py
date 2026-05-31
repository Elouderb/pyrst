class Student:
    def __init__(self, name: str, grade: int, score: float) -> None:
        self.name = name
        self.grade = grade
        self.score = score

    def is_passing(self) -> bool:
        return self.score >= 60.0

    def get_letter_grade(self) -> str:
        if self.score >= 90.0:
            return "A"
        elif self.score >= 80.0:
            return "B"
        elif self.score >= 70.0:
            return "C"
        elif self.score >= 60.0:
            return "D"
        else:
            return "F"

def main() -> None:
    # Create student roster
    students = [
        Student("Alice", 10, 95.5),
        Student("Bob", 10, 78.0),
        Student("Charlie", 10, 88.5),
        Student("Diana", 10, 92.0),
        Student("Eve", 10, 55.5),
        Student("Frank", 10, 87.0),
    ]

    # Count passing students
    passing = [s for s in students if s.is_passing()]
    print(len(passing))

    # Get high performers
    high_performers = [s for s in students if s.score > 85.0]
    print(len(high_performers))

    # Find students needing help
    struggling = [s for s in students if s.score < 70.0]
    print(len(struggling))

    # Calculate class average
    total_score = 0.0
    for student in students:
        total_score = total_score + student.score
    class_avg = total_score / len(students)

    print(class_avg)

    # Find highest and lowest scores
    scores = [s.score for s in students]
    highest = max(scores)
    lowest = min(scores)

    print(highest)
    print(lowest)

    # Letter grade distribution
    grade_a = [s for s in students if s.get_letter_grade() == "A"]
    grade_b = [s for s in students if s.get_letter_grade() == "B"]
    grade_c = [s for s in students if s.get_letter_grade() == "C"]
    grade_d = [s for s in students if s.get_letter_grade() == "D"]
    grade_f = [s for s in students if s.get_letter_grade() == "F"]

    print(len(grade_a))
    print(len(grade_b))
    print(len(grade_c))
    print(len(grade_d))
    print(len(grade_f))

    # Student names in sorted order
    names = [s.name for s in students]
    sorted_names = sorted(names)
    for name in sorted_names:
        print(name)

    # Grade report
    for student in students:
        letter = student.get_letter_grade()
        passing = student.is_passing()
        print(student.name)
        print(student.score)
        print(letter)
        print(passing)

    # Verification
    all_in_school = all([s.grade == 10 for s in students])
    print(all_in_school)

    # Top scorer name
    top_student = students[0]
    for student in students:
        if student.score > top_student.score:
            top_student = student
    print(top_student.name)
