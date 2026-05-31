# Temperature data logging and analysis

class DailyTemp:
    day: str
    high: float
    low: float
    humidity: float

    def __init__(self, day: str, high: float, low: float, humidity: float) -> None:
        self.day = day
        self.high = high
        self.low = low
        self.humidity = humidity

def main() -> None:
    temps = [
        DailyTemp("Monday", 75.0, 60.0, 65.0),
        DailyTemp("Tuesday", 78.0, 62.0, 60.0),
        DailyTemp("Wednesday", 72.0, 58.0, 70.0),
        DailyTemp("Thursday", 76.0, 61.0, 62.0),
        DailyTemp("Friday", 80.0, 65.0, 55.0),
    ]

    print("=== Temperature Log ===")
    print(len(temps))

    # High temperatures
    total_high = 0.0
    max_high = temps[0].high
    min_high = temps[0].high

    for day_temp in temps:
        total_high = total_high + day_temp.high
        if day_temp.high > max_high:
            max_high = day_temp.high
        if day_temp.high < min_high:
            min_high = day_temp.high

    print(total_high)
    print(max_high)
    print(min_high)

    avg_high = total_high / 5.0
    print(avg_high)

    # Low temperatures
    total_low = 0.0
    max_low = temps[0].low
    min_low = temps[0].low

    for day_temp in temps:
        total_low = total_low + day_temp.low
        if day_temp.low > max_low:
            max_low = day_temp.low
        if day_temp.low < min_low:
            min_low = day_temp.low

    print(total_low)
    print(max_low)
    print(min_low)

    avg_low = total_low / 5.0
    print(avg_low)

    # Humidity analysis
    total_humidity = 0.0
    high_humidity = 0

    for day_temp in temps:
        total_humidity = total_humidity + day_temp.humidity
        if day_temp.humidity > 65.0:
            high_humidity = high_humidity + 1

    avg_humidity = total_humidity / 5.0
    print(total_humidity)
    print(avg_humidity)
    print(high_humidity)

    # Warm days
    warm_count = 0
    for day_temp in temps:
        if day_temp.high > 75.0:
            warm_count = warm_count + 1

    print(warm_count)

    # Print daily details
    print("=== Daily Details ===")
    for day_temp in temps:
        print(day_temp.day)
        print(day_temp.high)
        print(day_temp.low)
        print(day_temp.humidity)

    # Temperature range
    print("=== Ranges ===")
    for day_temp in temps:
        range_val = day_temp.high - day_temp.low
        print(range_val)

    # Average range
    total_range = 0.0
    for day_temp in temps:
        total_range = total_range + (day_temp.high - day_temp.low)

    avg_range = total_range / 5.0
    print(avg_range)
