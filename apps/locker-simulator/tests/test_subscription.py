from types import SimpleNamespace
from unittest.mock import Mock, patch

from locker_simulator.__main__ import on_subscribe, subscribed


def test_simulator_announces_only_after_successful_suback() -> None:
    subscribed.clear()
    client = Mock()
    with patch('locker_simulator.__main__.publish_status') as publish:
        on_subscribe(client, None, 1, [SimpleNamespace(is_failure=True)], None)
        assert not subscribed.is_set()
        publish.assert_not_called()
        on_subscribe(client, None, 1, [SimpleNamespace(is_failure=False)], None)
        assert subscribed.is_set()
        publish.assert_called_once_with(client, 'LOCKER_ONLINE')
    subscribed.clear()
