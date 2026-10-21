"""Prove foreground precedence and cancellation without throughput timing guesses."""

from threading import Event, Thread

from core.task_scheduler import TaskScheduler


def test_overlapping_foreground_reservations_release_exactly_once():
    scheduler = TaskScheduler(settle_seconds=0)
    first, second = scheduler.foreground(), scheduler.foreground()
    entered, waiting = Event(), Event()

    def work():
        with scheduler.background_slot(
            "copy", Event(), lambda reason: waiting.set() if reason else None
        ):
            entered.set()

    worker = Thread(target=work)
    worker.start()
    try:
        assert waiting.wait(1)
        assert not entered.is_set()
        first.close()
        first.close()
        assert not entered.is_set()
        second.close()
        assert entered.wait(1)
    finally:
        first.close()
        second.close()
        worker.join(2)
    assert not worker.is_alive()


def test_same_lane_is_fifo_and_different_lanes_can_execute():
    scheduler = TaskScheduler(settle_seconds=0)
    first_running, release = Event(), Event()
    second_waiting, second_running, converter_running = Event(), Event(), Event()
    order = []

    def first():
        with scheduler.background_slot("copy", Event()):
            order.append("first")
            first_running.set()
            assert release.wait(3)

    def second():
        with scheduler.background_slot(
            "copy", Event(), lambda reason: second_waiting.set() if reason else None
        ):
            order.append("second")
            second_running.set()

    def converter():
        with scheduler.background_slot("dng", Event()):
            converter_running.set()

    workers = [Thread(target=fn) for fn in (first, second, converter)]
    try:
        workers[0].start()
        assert first_running.wait(1)
        workers[1].start()
        assert second_waiting.wait(1)
        workers[2].start()
        assert converter_running.wait(1)
        assert not second_running.is_set()
        release.set()
    finally:
        release.set()
        for worker in workers:
            if worker.ident:
                worker.join(3)
    assert order == ["first", "second"]
    assert all(not worker.is_alive() for worker in workers)


def test_cancelled_waiter_leaves_queue_without_ending_foreground_work():
    scheduler = TaskScheduler(settle_seconds=0)
    lease = scheduler.foreground()
    cancel, waiting = Event(), Event()
    cancelled = []

    def work():
        try:
            with scheduler.background_slot(
                "copy", cancel, lambda reason: waiting.set() if reason else None
            ):
                raise AssertionError("Background work must remain blocked")
        except InterruptedError:
            cancelled.append(True)

    worker = Thread(target=work)
    worker.start()
    try:
        assert waiting.wait(1)
        cancel.set()
        worker.join(2)
        assert not worker.is_alive()
        assert cancelled == [True]
        assert not scheduler._queues["copy"]
        assert scheduler._foreground == 1
    finally:
        lease.close()
        worker.join(2)


def test_failed_background_job_releases_its_execution_slot():
    scheduler = TaskScheduler(settle_seconds=0)
    try:
        with scheduler.background_slot("dng", Event()):
            raise ValueError("converter failed")
    except ValueError:
        pass
    with scheduler.background_slot("dng", Event()):
        assert scheduler._active["dng"] == 1
    assert scheduler._active["dng"] == 0
