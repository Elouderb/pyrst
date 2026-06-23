# Weather data analysis with data structures and calculations

class DayWeather:
    day: str
    temp_high: float
    temp_low: float
    precipitation: float

    def __init__(self, day: str, temp_high: float, temp_low: float, precipitation: float) -> None:
        self.day = day
        self.temp_high = temp_high
        self.temp_low = temp_low
        self.precipitation = precipitation

    def get_avg_temp(self) -> float:
        return (self.temp_high + self.temp_low) / 2.0

    def is_rainy(self) -> bool:
        return self.precipitation > 0.0

def main() -> None:
    # Create weekly weather data
    week = [
        DayWeather("Monday", 75.0, 60.0, 0.0),
        DayWeather("Tuesday", 78.0, 62.0, 0.5),
        DayWeather("Wednesday", 72.0, 58.0, 1.2),
        DayWeather("Thursday", 76.0, 61.0, 0.0),
        DayWeather("Friday", 80.0, 65.0, 0.0),
        DayWeather("Saturday", 82.0, 67.0, 0.1),
        DayWeather("Sunday", 79.0, 64.0, 0.3),
    ]

    print("=== Weather Analysis ===")
    print(len(week))

    # Temperature analysis
    highs = [d.temp_high for d in week]
    lows = [d.temp_low for d in week]
    max_high = max(highs)
    min_low = min(lows)
    avg_high = sum(highs) / len(highs)
    avg_low = sum(lows) / len(lows)

    print(max_high)
    print(min_low)
    print(avg_high)
    print(avg_low)

    # Daily average temperatures
    avg_temps = [d.get_avg_temp() for d in week]
    week_avg = sum(avg_temps) / len(avg_temps)

    print(week_avg)
    print(max(avg_temps))
    print(min(avg_temps))

    # Precipitation analysis
    precipitations = [d.precipitation for d in week]
    total_precip = sum(precipitations)
    max_precip = max(precipitations)
    num_rainy = len([d for d in week if d.is_rainy()])
    num_clear = len([d for d in week if not d.is_rainy()])

    print(total_precip)
    print(max_precip)
    print(num_rainy)
    print(num_clear)

    # Temperature ranges
    temp_ranges = [d.temp_high - d.temp_low for d in week]
    avg_range = sum(temp_ranges) / len(temp_ranges)

    print(avg_range)

    # Find warmest and coldest days
    warmest_high = max(highs)
    coldest_low = min(lows)

    print(warmest_high)
    print(coldest_low)

    # Print day details
    print("=== Daily Details ===")
    for day in week:
        print(day.day)
        avg = day.get_avg_temp()
        print(avg)
        print(day.precipitation)

    # Sorted by average temperature
    sorted_days = sorted(week, key=lambda d: d.get_avg_temp())
    print("=== Sorted by Avg Temp ===")
    for d in sorted_days:
        print(d.day)

    # Temperature categories
    hot_days = [d for d in week if d.temp_high > 78.0]
    cool_days = [d for d in week if d.temp_low < 62.0]

    print(len(hot_days))
    print(len(cool_days))
