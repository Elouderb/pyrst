# Grade processing system with classes and statistical analysis

class Student:
    name: str
    grade: int
    scores: list[int]

    def __init__(self, name: str, grade: int) -> None:
        self.name = name
        self.grade = grade
        self.scores = []

    def add_score(self, score: int) -> None:
        self.scores.append(score)

    def get_average(self) -> float:
        if len(self.scores) == 0:
            return 0.0
        return sum(self.scores) / len(self.scores)

    def get_highest(self) -> int:
        if len(self.scores) == 0:
            return 0
        return max(self.scores)

    def get_lowest(self) -> int:
        if len(self.scores) == 0:
            return 0
        return min(self.scores)

def main() -> None:
    # Create students
    student1 = Student("Alice", 10)
    student1.add_score(95)
    student1.add_score(88)
    student1.add_score(92)

    student2 = Student("Bob", 10)
    student2.add_score(78)
    student2.add_score(82)
    student2.add_score(80)

    student3 = Student("Charlie", 10)
    student3.add_score(88)
    student3.add_score(91)
    student3.add_score(87)

    students = [student1, student2, student3]

    print("=== Grade Processing ===")
    print(len(students))

    # Calculate averages
    averages = [s.get_average() for s in students]
    total_avg = sum(averages) / len(averages)
    max_avg = max(averages)
    min_avg = min(averages)

    print(total_avg)
    print(max_avg)
    print(min_avg)

    # Highest and lowest scores
    highest_scores = [s.get_highest() for s in students]
    lowest_scores = [s.get_lowest() for s in students]

    print(sum(highest_scores))
    print(sum(lowest_scores))

    # Score ranges
    score_ranges = [s.get_highest() - s.get_lowest() for s in students]
    print(sum(score_ranges))

    # Total scores
    all_scores = []
    for s in students:
        for score in s.scores:
            all_scores.append(score)

    print(len(all_scores))
    print(sum(all_scores))
    print(max(all_scores))
    print(min(all_scores))

    # Print details
    print("=== Student Details ===")
    for student in students:
        print(student.name)
        avg = student.get_average()
        print(avg)
        high = student.get_highest()
        print(high)
        low = student.get_lowest()
        print(low)

    # Sorted by average
    sorted_students = sorted(students, key=lambda s: s.get_average())
    print("=== Sorted by Average ===")
    for s in sorted_students:
        print(s.name)

    # Grade analysis
    num_scores = sum([len(s.scores) for s in students])
    print(num_scores)
