"""Application entry point."""

from pkg.util.helper import Helper, shape_area


class Runner:
    """Runs the app."""

    def run(self):
        helper = Helper()
        area = helper.format(2.0)
        return shape_area(area)
