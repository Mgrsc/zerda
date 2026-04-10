from __future__ import annotations

import os
import sys
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[2]
PRIMITIVES_ROOT = ROOT / "code_primitives" / "python"

for candidate in (ROOT, PRIMITIVES_ROOT):
    value = str(candidate)
    if value not in sys.path:
        sys.path.insert(0, value)

from custom_primitives.catalog import get_primitive_registry
from custom_primitives.divination import primitive


class FakeGZ:
    def __init__(self, tg: int, dz: int) -> None:
        self.tg = tg
        self.dz = dz


class FakeDay:
    def __init__(
        self,
        *,
        solar_year: int,
        solar_month: int,
        solar_day: int,
        lunar_year: int,
        lunar_month: int,
        lunar_day: int,
        is_leap_month: bool,
        year_gz: FakeGZ,
        month_gz: FakeGZ,
        day_gz: FakeGZ,
        hour_gz: FakeGZ,
    ) -> None:
        self._solar_year = solar_year
        self._solar_month = solar_month
        self._solar_day = solar_day
        self._lunar_year = lunar_year
        self._lunar_month = lunar_month
        self._lunar_day = lunar_day
        self._is_leap_month = is_leap_month
        self._year_gz = year_gz
        self._month_gz = month_gz
        self._day_gz = day_gz
        self._hour_gz = hour_gz

    def getSolarYear(self) -> int:
        return self._solar_year

    def getSolarMonth(self) -> int:
        return self._solar_month

    def getSolarDay(self) -> int:
        return self._solar_day

    def getLunarYear(self, _boundary: bool = False) -> int:
        return self._lunar_year

    def getLunarMonth(self) -> int:
        return self._lunar_month

    def getLunarDay(self) -> int:
        return self._lunar_day

    def isLunarLeap(self) -> bool:
        return self._is_leap_month

    def getYearGZ(self, _boundary: bool = False) -> FakeGZ:
        return self._year_gz

    def getMonthGZ(self) -> FakeGZ:
        return self._month_gz

    def getDayGZ(self) -> FakeGZ:
        return self._day_gz

    def getHourGZ(self, _hour: int) -> FakeGZ:
        return self._hour_gz


class FakeSxtwl:
    def fromSolar(self, year: int, month: int, day: int) -> FakeDay:
        if (year, month, day) == (2024, 2, 10):
            return FakeDay(
                solar_year=2024,
                solar_month=2,
                solar_day=10,
                lunar_year=2024,
                lunar_month=1,
                lunar_day=1,
                is_leap_month=False,
                year_gz=FakeGZ(0, 4),
                month_gz=FakeGZ(2, 2),
                day_gz=FakeGZ(3, 5),
                hour_gz=FakeGZ(4, 6),
            )
        raise AssertionError(f"unexpected solar date {(year, month, day)}")

    def fromLunar(
        self,
        year: int,
        month: int,
        day: int,
        is_leap_month: bool = False,
    ) -> FakeDay:
        if (year, month, day, is_leap_month) == (2024, 1, 1, False):
            return FakeDay(
                solar_year=2024,
                solar_month=2,
                solar_day=10,
                lunar_year=2024,
                lunar_month=1,
                lunar_day=1,
                is_leap_month=False,
                year_gz=FakeGZ(0, 4),
                month_gz=FakeGZ(2, 2),
                day_gz=FakeGZ(3, 5),
                hour_gz=FakeGZ(4, 6),
            )
        raise AssertionError(
            f"unexpected lunar date {(year, month, day, is_leap_month)}"
        )


class DivinationPrimitiveTests(unittest.IsolatedAsyncioTestCase):
    def setUp(self) -> None:
        self.temp_dir = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp_dir.cleanup)
        self.env_patch = patch.dict(
            os.environ,
            {
                "HOME": self.temp_dir.name,
                "PTC_SESSION_KEY": "cli:test-user",
            },
            clear=False,
        )
        self.env_patch.start()
        self.addCleanup(self.env_patch.stop)

    async def test_catalog_registers_divination_primitives(self) -> None:
        registry = get_primitive_registry()
        self.assertIn("get_lunar_info", registry)
        self.assertIn("convert_lunar_to_solar", registry)
        self.assertIn("get_sizhu_info", registry)
        self.assertIn("calculate_meihua", registry)

    async def test_get_lunar_info_returns_dependency_missing_without_sxtwl(self) -> None:
        with patch.object(primitive, "_SXTWL", None):
            result = await primitive.get_lunar_info(datetime_str="2024-02-10 12:00:00")
        self.assertEqual(result["status"], "dependency_missing")
        self.assertEqual(result["error_code"], "missing_dependency")
        self.assertIn("sxtwl", result["error_message"])

    async def test_get_lunar_info_reports_installing_state(self) -> None:
        cache_dir = Path(self.temp_dir.name) / ".zerda" / "custom_primitives"
        cache_dir.mkdir(parents=True, exist_ok=True)
        (cache_dir / "install.state").write_text("installing\n", encoding="utf-8")
        with patch.object(primitive, "_SXTWL", None):
            result = await primitive.get_lunar_info(datetime_str="2024-02-10 12:00:00")
        self.assertEqual(result["status"], "dependency_missing")
        self.assertEqual(result["error_code"], "missing_dependency")
        self.assertIn("still installing", result["error_message"])

    async def test_get_lunar_info_formats_lunar_payload(self) -> None:
        with patch.object(primitive, "_SXTWL", FakeSxtwl()):
            result = await primitive.get_lunar_info(datetime_str="2024-02-10 12:00:00")
        self.assertEqual(result["status"], "ok")
        self.assertEqual(result["data"]["lunar_year"], 2024)
        self.assertEqual(result["data"]["lunar_month"], 1)
        self.assertEqual(result["data"]["lunar_day"], 1)
        self.assertEqual(result["data"]["lunar_month_cn"], "正月")
        self.assertEqual(result["data"]["lunar_day_cn"], "初一")
        self.assertEqual(result["data"]["lunar_time_cn"], "2024年正月初一")

    async def test_convert_lunar_to_solar_returns_gregorian_payload(self) -> None:
        with patch.object(primitive, "_SXTWL", FakeSxtwl()):
            result = await primitive.convert_lunar_to_solar(
                lunar_year=2024,
                lunar_month=1,
                lunar_day=1,
            )
        self.assertEqual(result["status"], "ok")
        self.assertEqual(result["data"]["gregorian_year"], 2024)
        self.assertEqual(result["data"]["gregorian_month"], 2)
        self.assertEqual(result["data"]["gregorian_day"], 10)
        self.assertEqual(result["data"]["gregorian_date"], "2024-02-10")

    async def test_get_sizhu_info_returns_four_pillars(self) -> None:
        with patch.object(primitive, "_SXTWL", FakeSxtwl()):
            result = await primitive.get_sizhu_info(datetime_str="2024-02-10 12:00:00")
        self.assertEqual(result["status"], "ok")
        self.assertEqual(result["data"]["year_pillar"], "甲辰")
        self.assertEqual(result["data"]["month_pillar"], "丙寅")
        self.assertEqual(result["data"]["day_pillar"], "丁巳")
        self.assertEqual(result["data"]["hour_pillar"], "戊午")
        self.assertEqual(
            result["data"]["sizhu_cn"],
            "甲辰年 丙寅月 丁巳日 戊午时",
        )

    async def test_calculate_meihua_time_mode_returns_gua_payload(self) -> None:
        with patch.object(primitive, "_SXTWL", FakeSxtwl()):
            result = await primitive.calculate_meihua(
                method=1,
                datetime_str="2024-02-10 12:00:00",
            )
        self.assertEqual(result["status"], "ok")
        self.assertEqual(result["data"]["method"], 1)
        self.assertEqual(result["data"]["method_name"], "time")
        self.assertEqual(result["data"]["ben_gua"], "艮坎")
        self.assertEqual(result["data"]["bian_gua"], "艮坤")
        self.assertEqual(result["data"]["hu_gua"], "坤震")
        self.assertEqual(result["data"]["dong_yao"], 2)

    async def test_calculate_meihua_random_mode_returns_random_values(self) -> None:
        with patch.object(primitive, "_SXTWL", FakeSxtwl()):
            with patch.object(primitive.secrets, "randbelow", side_effect=[0, 7, 4]):
                result = await primitive.calculate_meihua(method=3)
        self.assertEqual(result["status"], "ok")
        self.assertEqual(result["data"]["method"], 3)
        self.assertEqual(result["data"]["method_name"], "random")
        self.assertEqual(result["data"]["ben_gua"], "乾坤")
        self.assertEqual(result["data"]["bian_gua"], "巽坤")
        self.assertEqual(result["data"]["hu_gua"], "离艮")
        self.assertEqual(result["data"]["dong_yao"], 5)
        self.assertEqual(
            result["data"]["random_values"],
            {"upper": 0, "lower": 7, "yao": 5},
        )

    async def test_calculate_meihua_rejects_invalid_method(self) -> None:
        result = await primitive.calculate_meihua(method=2)
        self.assertEqual(result["status"], "invalid_argument")
        self.assertEqual(result["error_code"], "invalid_argument")


if __name__ == "__main__":
    unittest.main()
