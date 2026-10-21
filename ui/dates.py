"""Locale-independent German date labels."""
from datetime import date

MONTHS = ('Januar', 'Februar', 'März', 'April', 'Mai', 'Juni',
          'Juli', 'August', 'September', 'Oktober', 'November', 'Dezember')


def date_label(value: str) -> str:
    """Format an ISO date for the interface."""
    day = date.fromisoformat(value)
    return f'{day.day:02d}. {MONTHS[day.month - 1]} {day.year}'
