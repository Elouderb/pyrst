# Project tracking and time management

class Task:
    name: str
    hours: float
    priority: float
    completed: float

    def __init__(self, name: str, hours: float, priority: float, completed: float) -> None:
        self.name = name
        self.hours = hours
        self.priority = priority
        self.completed = completed

def main() -> None:
    tasks = [
        Task("Design", 8.0, 1.0, 1.0),
        Task("Development", 40.0, 1.0, 0.5),
        Task("Testing", 16.0, 2.0, 0.0),
        Task("Documentation", 8.0, 3.0, 0.75),
        Task("Deployment", 4.0, 1.0, 0.0),
    ]

    print("=== Project Tracker ===")
    print(len(tasks))

    # Total hours
    total_hours = 0.0
    for task in tasks:
        total_hours = total_hours + task.hours

    print(total_hours)

    # Completed hours
    completed_hours = 0.0
    for task in tasks:
        task_completed = task.hours * task.completed
        completed_hours = completed_hours + task_completed

    print(completed_hours)

    # Remaining hours
    remaining_hours = total_hours - completed_hours
    print(remaining_hours)

    # Progress percentage
    progress_pct = (completed_hours / total_hours) * 100.0
    print(progress_pct)

    # High priority tasks
    high_priority = 0
    for task in tasks:
        if task.priority <= 1.0:
            high_priority = high_priority + 1

    print(high_priority)

    # Heavy tasks
    heavy_tasks = 0
    for task in tasks:
        if task.hours >= 16.0:
            heavy_tasks = heavy_tasks + 1

    print(heavy_tasks)

    # Max and min hours
    max_hours = tasks[0].hours
    min_hours = tasks[0].hours

    for task in tasks:
        if task.hours > max_hours:
            max_hours = task.hours
        if task.hours < min_hours:
            min_hours = task.hours

    print(max_hours)
    print(min_hours)

    # Average task hours
    avg_hours = total_hours / 5.0
    print(avg_hours)

    # Priority analysis
    p1_hours = 0.0
    p2_hours = 0.0
    p3_hours = 0.0

    for task in tasks:
        if task.priority == 1.0:
            p1_hours = p1_hours + task.hours
        else:
            if task.priority == 2.0:
                p2_hours = p2_hours + task.hours
            else:
                p3_hours = p3_hours + task.hours

    print(p1_hours)
    print(p2_hours)
    print(p3_hours)

    # Print all tasks
    print("=== Task Details ===")
    for task in tasks:
        print(task.name)
        print(task.hours)
        print(task.priority)
        print(task.completed)

    # Hours per priority
    print("=== Hours by Priority ===")
    for task in tasks:
        remaining = task.hours * (1.0 - task.completed)
        print(remaining)

    # Task status summary
    completed_tasks = 0
    in_progress = 0
    not_started = 0

    for task in tasks:
        if task.completed == 1.0:
            completed_tasks = completed_tasks + 1
        else:
            if task.completed > 0.0:
                in_progress = in_progress + 1
            else:
                not_started = not_started + 1

    print(completed_tasks)
    print(in_progress)
    print(not_started)
