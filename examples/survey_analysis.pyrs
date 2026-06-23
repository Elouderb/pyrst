# Survey response analysis

class Response:
    respondent: str
    rating1: float
    rating2: float
    rating3: float
    rating4: float

    def __init__(self, respondent: str, rating1: float, rating2: float, rating3: float, rating4: float) -> None:
        self.respondent = respondent
        self.rating1 = rating1
        self.rating2 = rating2
        self.rating3 = rating3
        self.rating4 = rating4

def main() -> None:
    responses = [
        Response("Person1", 5.0, 4.0, 5.0, 3.0),
        Response("Person2", 4.0, 3.0, 4.0, 4.0),
        Response("Person3", 5.0, 5.0, 5.0, 5.0),
        Response("Person4", 3.0, 3.0, 3.0, 2.0),
        Response("Person5", 4.0, 4.0, 4.0, 4.0),
    ]

    print("=== Survey Analysis ===")
    print(len(responses))

    # Question 1 analysis
    total_q1 = 0.0
    max_q1 = responses[0].rating1
    min_q1 = responses[0].rating1

    for resp in responses:
        total_q1 = total_q1 + resp.rating1
        if resp.rating1 > max_q1:
            max_q1 = resp.rating1
        if resp.rating1 < min_q1:
            min_q1 = resp.rating1

    avg_q1 = total_q1 / 5.0
    print(total_q1)
    print(avg_q1)
    print(max_q1)
    print(min_q1)

    # Question 2 analysis
    total_q2 = 0.0
    for resp in responses:
        total_q2 = total_q2 + resp.rating2

    avg_q2 = total_q2 / 5.0
    print(total_q2)
    print(avg_q2)

    # Question 3 analysis
    total_q3 = 0.0
    for resp in responses:
        total_q3 = total_q3 + resp.rating3

    avg_q3 = total_q3 / 5.0
    print(total_q3)
    print(avg_q3)

    # Question 4 analysis
    total_q4 = 0.0
    for resp in responses:
        total_q4 = total_q4 + resp.rating4

    avg_q4 = total_q4 / 5.0
    print(total_q4)
    print(avg_q4)

    # Overall satisfaction (average of all ratings)
    all_total = total_q1 + total_q2 + total_q3 + total_q4
    overall_avg = all_total / 20.0
    print(all_total)
    print(overall_avg)

    # High satisfaction (all ratings >= 4)
    high_satisfaction = 0
    for resp in responses:
        if resp.rating1 >= 4.0:
            if resp.rating2 >= 4.0:
                if resp.rating3 >= 4.0:
                    if resp.rating4 >= 4.0:
                        high_satisfaction = high_satisfaction + 1

    print(high_satisfaction)

    # Print all responses
    print("=== Responses ===")
    for resp in responses:
        print(resp.respondent)
        print(resp.rating1)
        print(resp.rating2)
        print(resp.rating3)
        print(resp.rating4)

    # Average per respondent
    print("=== Individual Averages ===")
    for resp in responses:
        individual_avg = (resp.rating1 + resp.rating2 + resp.rating3 + resp.rating4) / 4.0
        print(individual_avg)

    # Question difficulty (inverse of average)
    print("=== Question Analysis ===")
    q_avgs = [avg_q1, avg_q2, avg_q3, avg_q4]
    print(avg_q1)
    print(avg_q2)
    print(avg_q3)
    print(avg_q4)

    # Comparison
    q1_vs_q2 = avg_q1 - avg_q2
    q3_vs_q4 = avg_q3 - avg_q4
    print(q1_vs_q2)
    print(q3_vs_q4)

    # Summary
    total_responses = len(responses)
    print(total_responses)
    print(overall_avg)
