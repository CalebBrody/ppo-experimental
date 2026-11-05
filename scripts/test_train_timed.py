"""Windows status-write sharing violations must not kill healthy trainers."""
import json
import unittest
from unittest.mock import Mock, patch
from train_timed import write_json


class StatusWriteTests(unittest.TestCase):
    def test_transient_sharing_violation_is_retried(self):
        path=Mock(); temp=path.with_suffix.return_value
        temp.replace.side_effect=[PermissionError('shared reader'),PermissionError('scanner'),None]
        with patch('train_timed.time.sleep') as sleep:
            write_json(path,{'status':'training'})
        self.assertEqual(temp.replace.call_count,3)
        self.assertEqual(sleep.call_count,2)
        self.assertEqual(json.loads(temp.write_text.call_args.args[0]),{'status':'training'})

    def test_persistent_permission_problem_is_not_hidden(self):
        path=Mock(); path.with_suffix.return_value.replace.side_effect=PermissionError('persistent')
        with patch('train_timed.time.sleep'),self.assertRaises(PermissionError):
            write_json(path,{'status':'training'})
        self.assertEqual(path.with_suffix.return_value.replace.call_count,40)


if __name__=='__main__':unittest.main()
