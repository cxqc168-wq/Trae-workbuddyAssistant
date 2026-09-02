#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""auto_checkin 纯函数单测（无需网络）。
可用 `python src-python/tests/test_auto_checkin.py` 直接运行，或 `pytest src-python/tests/`。
"""
import base64
import json
import time
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))

from auto_checkin import extract_user_id, get_jwt_exp, rand_digits


def _make_jwt(user_id, exp_offset_hours=72):
    header = base64.urlsafe_b64encode(json.dumps({"alg": "RS256", "typ": "JWT"}).encode()).rstrip(b"=")
    payload = base64.urlsafe_b64encode(
        json.dumps({"data": {"id": user_id}, "exp": int(time.time()) + exp_offset_hours * 3600}).encode()
    ).rstrip(b"=")
    return f"Cloud-IDE-JWT {header.decode()}.{payload.decode()}.sig"


def test_extract_user_id():
    uid = "1234567890123456"
    assert extract_user_id(_make_jwt(uid)) == uid
    # 无前缀也能解析
    assert extract_user_id(_make_jwt(uid).split(" ", 1)[1]) == uid
    # 非法 token 返回 None
    assert extract_user_id("not-a-jwt") is None


def test_get_jwt_exp():
    uid = "1234567890123456"
    exp_dt, remain = get_jwt_exp(_make_jwt(uid, 48))
    assert exp_dt is not None
    assert remain is not None
    assert 47 < remain < 49


def test_rand_digits_deterministic():
    a = rand_digits(15, seed="1234567890123456")
    b = rand_digits(15, seed="1234567890123456")
    assert a == b
    assert len(a) == 15
    assert a.isdigit()


if __name__ == "__main__":
    test_extract_user_id()
    test_get_jwt_exp()
    test_rand_digits_deterministic()
    print("ALL TESTS PASSED")
