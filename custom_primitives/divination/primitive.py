from __future__ import annotations

import datetime as dt
import importlib
import os
import secrets
from typing import Any
from zoneinfo import ZoneInfo

from primitives.base import (
    dependency_missing_result,
    invalid_argument_result,
    load_context,
    run_with_guard,
    validate_int_range,
)
from primitives.types import ActionStatus, PrimitiveResult

try:
    import sxtwl as _SXTWL
except Exception:
    _SXTWL = None

CHINA_TZ = ZoneInfo("Asia/Shanghai")
HEAVENLY_STEMS = ["甲", "乙", "丙", "丁", "戊", "己", "庚", "辛", "壬", "癸"]
EARTHLY_BRANCHES = ["子", "丑", "寅", "卯", "辰", "巳", "午", "未", "申", "酉", "戌", "亥"]
LUNAR_MONTHS_CN = ["正", "二", "三", "四", "五", "六", "七", "八", "九", "十", "十一", "十二"]
LUNAR_DAYS_CN = [
    "初一",
    "初二",
    "初三",
    "初四",
    "初五",
    "初六",
    "初七",
    "初八",
    "初九",
    "初十",
    "十一",
    "十二",
    "十三",
    "十四",
    "十五",
    "十六",
    "十七",
    "十八",
    "十九",
    "二十",
    "廿一",
    "廿二",
    "廿三",
    "廿四",
    "廿五",
    "廿六",
    "廿七",
    "廿八",
    "廿九",
    "三十",
    "卅一",
]
EIGHT_TRIGRAMS = ["乾", "兑", "离", "震", "巽", "坎", "艮", "坤"]
TRIGRAM_BINARY = {
    "坤": [0, 0, 0],
    "震": [1, 0, 0],
    "坎": [0, 1, 0],
    "兑": [1, 1, 0],
    "艮": [0, 0, 1],
    "巽": [1, 0, 1],
    "离": [0, 1, 1],
    "乾": [1, 1, 1],
}
BINARY_TRIGRAM = {tuple(value): key for key, value in TRIGRAM_BINARY.items()}
DATETIME_FORMAT = "%Y-%m-%d %H:%M:%S"


def _resolve_datetime(datetime_str: str | None = None) -> dt.datetime:
    if datetime_str is None:
        return dt.datetime.now(CHINA_TZ)
    parsed = dt.datetime.strptime(datetime_str, DATETIME_FORMAT)
    return parsed.replace(tzinfo=CHINA_TZ)


def _format_gregorian_time(value: dt.datetime) -> str:
    return value.astimezone(CHINA_TZ).strftime(DATETIME_FORMAT)


def _custom_primitives_cache_root() -> str:
    return os.environ.get(
        "ZERDA_CUSTOM_PRIMITIVES_CACHE_DIR",
        os.path.join(os.path.expanduser("~"), ".zerda", "custom_primitives"),
    )


def _install_state_path() -> str:
    return os.path.join(_custom_primitives_cache_root(), "install.state")


def _read_install_state() -> str:
    try:
        with open(_install_state_path(), encoding="utf-8") as handle:
            return handle.read().strip()
    except OSError:
        return ""


def _require_sxtwl() -> Any | PrimitiveResult:
    global _SXTWL
    if _SXTWL is None:
        try:
            _SXTWL = importlib.import_module("sxtwl")
        except Exception:
            if _read_install_state() == "installing":
                return dependency_missing_result(
                    "sxtwl is still installing in the custom primitives environment; retry shortly"
                )
            return dependency_missing_result(
                "Missing sxtwl runtime; install sxtwl before using divination primitives"
            )
    return _SXTWL


def _get_trigram_by_number(number: int) -> str:
    return EIGHT_TRIGRAMS[number - 1]


def _get_interacting_gua(up: str, low: str) -> tuple[str | None, str | None]:
    lines = TRIGRAM_BINARY[low] + TRIGRAM_BINARY[up]
    lower = lines[1:4]
    upper = lines[2:5]
    return BINARY_TRIGRAM.get(tuple(upper)), BINARY_TRIGRAM.get(tuple(lower))


def _process_divination_result(up: str, low: str, moving_yao: int) -> dict[str, object]:
    hu_up, hu_low = _get_interacting_gua(up, low)
    original_hex = TRIGRAM_BINARY[low] + TRIGRAM_BINARY[up]
    mutated_hex = list(original_hex)
    mutated_hex[moving_yao - 1] = 1 - mutated_hex[moving_yao - 1]
    bian_low = BINARY_TRIGRAM.get(tuple(mutated_hex[:3]))
    bian_up = BINARY_TRIGRAM.get(tuple(mutated_hex[3:]))
    return {
        "ben_gua": up + low,
        "bian_gua": f"{bian_up or ''}{bian_low or ''}",
        "hu_gua": f"{hu_up or ''}{hu_low or ''}",
        "dong_yao": moving_yao,
    }


def _time_based_divination(
    year_number: int,
    lunar_month: int,
    lunar_day: int,
    hour_number: int,
) -> dict[str, object]:
    upper_num = (year_number + lunar_month + lunar_day) % 8 or 8
    lower_num = (year_number + lunar_month + lunar_day + hour_number) % 8 or 8
    moving_yao = (year_number + lunar_month + lunar_day + hour_number) % 6 or 6
    return _process_divination_result(
        _get_trigram_by_number(upper_num),
        _get_trigram_by_number(lower_num),
        moving_yao,
    )


def _resolve_meihua_time_numbers(sxtwl_module: Any, datetime_str: str | None) -> tuple[int, int]:
    current = _resolve_datetime(datetime_str)
    day = sxtwl_module.fromSolar(current.year, current.month, current.day)
    year_number = day.getYearGZ().dz + 1
    hour_number = day.getHourGZ(current.hour % 24).dz + 1
    return year_number, hour_number


def _get_lunar_details(sxtwl_module: Any, datetime_str: str | None) -> dict[str, object]:
    current = _resolve_datetime(datetime_str)
    day = sxtwl_module.fromSolar(current.year, current.month, current.day)
    lunar_year = day.getLunarYear(False)
    lunar_month = day.getLunarMonth()
    lunar_day = day.getLunarDay()
    is_leap_month = day.isLunarLeap()
    lunar_month_cn = f"{'闰' if is_leap_month else ''}{LUNAR_MONTHS_CN[lunar_month - 1]}月"
    lunar_day_cn = LUNAR_DAYS_CN[lunar_day - 1]
    return {
        "gregorian_time": _format_gregorian_time(current),
        "lunar_year": lunar_year,
        "lunar_month": lunar_month,
        "lunar_day": lunar_day,
        "is_leap_month": is_leap_month,
        "lunar_month_cn": lunar_month_cn,
        "lunar_day_cn": lunar_day_cn,
        "lunar_time_cn": f"{lunar_year}年{lunar_month_cn}{lunar_day_cn}",
    }


def _convert_lunar_to_solar_details(
    sxtwl_module: Any,
    lunar_year: int,
    lunar_month: int,
    lunar_day: int,
    is_leap_month: bool,
) -> dict[str, object]:
    day = sxtwl_module.fromLunar(lunar_year, lunar_month, lunar_day, is_leap_month)
    gregorian_year = day.getSolarYear()
    gregorian_month = day.getSolarMonth()
    gregorian_day = day.getSolarDay()
    return {
        "lunar_year": lunar_year,
        "lunar_month": lunar_month,
        "lunar_day": lunar_day,
        "is_leap_month": is_leap_month,
        "gregorian_year": gregorian_year,
        "gregorian_month": gregorian_month,
        "gregorian_day": gregorian_day,
        "gregorian_date": (
            f"{gregorian_year:04d}-{gregorian_month:02d}-{gregorian_day:02d}"
        ),
    }


def _get_sizhu_details(sxtwl_module: Any, datetime_str: str | None) -> dict[str, str]:
    current = _resolve_datetime(datetime_str)
    day = sxtwl_module.fromSolar(current.year, current.month, current.day)
    year_gz = day.getYearGZ()
    month_gz = day.getMonthGZ()
    day_gz = day.getDayGZ()
    hour_gz = day.getHourGZ(current.hour % 24)
    year_pillar = f"{HEAVENLY_STEMS[year_gz.tg]}{EARTHLY_BRANCHES[year_gz.dz]}"
    month_pillar = f"{HEAVENLY_STEMS[month_gz.tg]}{EARTHLY_BRANCHES[month_gz.dz]}"
    day_pillar = f"{HEAVENLY_STEMS[day_gz.tg]}{EARTHLY_BRANCHES[day_gz.dz]}"
    hour_pillar = f"{HEAVENLY_STEMS[hour_gz.tg]}{EARTHLY_BRANCHES[hour_gz.dz]}"
    return {
        "gregorian_time": _format_gregorian_time(current),
        "year_pillar": year_pillar,
        "month_pillar": month_pillar,
        "day_pillar": day_pillar,
        "hour_pillar": hour_pillar,
        "sizhu_cn": f"{year_pillar}年 {month_pillar}月 {day_pillar}日 {hour_pillar}时",
    }


def _calculate_time_meihua(sxtwl_module: Any, datetime_str: str | None) -> dict[str, object]:
    lunar_payload = _get_lunar_details(sxtwl_module, datetime_str)
    sizhu_payload = _get_sizhu_details(sxtwl_module, datetime_str)
    year_number, hour_number = _resolve_meihua_time_numbers(sxtwl_module, datetime_str)
    result = _time_based_divination(
        year_number,
        int(lunar_payload["lunar_month"]),
        int(lunar_payload["lunar_day"]),
        hour_number,
    )
    return {
        "method": 1,
        "method_name": "time",
        "gregorian_time": str(lunar_payload["gregorian_time"]),
        "lunar_time_cn": str(lunar_payload["lunar_time_cn"]),
        "sizhu_cn": sizhu_payload["sizhu_cn"],
        "ben_gua": result["ben_gua"],
        "bian_gua": result["bian_gua"],
        "hu_gua": result["hu_gua"],
        "dong_yao": result["dong_yao"],
        "random_values": None,
    }


def _calculate_random_meihua() -> dict[str, object]:
    upper_index = secrets.randbelow(8)
    lower_index = secrets.randbelow(8)
    yao_index = secrets.randbelow(6) + 1
    result = _process_divination_result(
        EIGHT_TRIGRAMS[upper_index],
        EIGHT_TRIGRAMS[lower_index],
        yao_index,
    )
    return {
        "method": 3,
        "method_name": "random",
        "gregorian_time": None,
        "lunar_time_cn": None,
        "sizhu_cn": None,
        "ben_gua": result["ben_gua"],
        "bian_gua": result["bian_gua"],
        "hu_gua": result["hu_gua"],
        "dong_yao": result["dong_yao"],
        "random_values": {
            "upper": upper_index,
            "lower": lower_index,
            "yao": yao_index,
        },
    }


def _lunar_operation(datetime_str: str | None) -> PrimitiveResult:
    sxtwl_module = _require_sxtwl()
    if isinstance(sxtwl_module, PrimitiveResult):
        return sxtwl_module
    return PrimitiveResult(
        status=ActionStatus.OK,
        data=_get_lunar_details(sxtwl_module, datetime_str),
    )


def _lunar_to_solar_operation(
    lunar_year: int,
    lunar_month: int,
    lunar_day: int,
    is_leap_month: bool,
) -> PrimitiveResult:
    sxtwl_module = _require_sxtwl()
    if isinstance(sxtwl_module, PrimitiveResult):
        return sxtwl_module
    return PrimitiveResult(
        status=ActionStatus.OK,
        data=_convert_lunar_to_solar_details(
            sxtwl_module,
            lunar_year,
            lunar_month,
            lunar_day,
            is_leap_month,
        ),
    )


def _sizhu_operation(datetime_str: str | None) -> PrimitiveResult:
    sxtwl_module = _require_sxtwl()
    if isinstance(sxtwl_module, PrimitiveResult):
        return sxtwl_module
    return PrimitiveResult(
        status=ActionStatus.OK,
        data=_get_sizhu_details(sxtwl_module, datetime_str),
    )


def _meihua_operation(method: int, datetime_str: str | None) -> PrimitiveResult:
    if method == 3:
        return PrimitiveResult(status=ActionStatus.OK, data=_calculate_random_meihua())
    sxtwl_module = _require_sxtwl()
    if isinstance(sxtwl_module, PrimitiveResult):
        return sxtwl_module
    return PrimitiveResult(
        status=ActionStatus.OK,
        data=_calculate_time_meihua(sxtwl_module, datetime_str),
    )


async def get_lunar_info(datetime_str: str | None = None) -> dict[str, Any]:
    """
    [What it does]
    Converts a Gregorian datetime into lunar calendar details in China time.

    [Args]
    datetime_str: Datetime string in YYYY-MM-DD HH:MM:SS format. Uses current China time when omitted.

    [Output Contract]
    res = await get_lunar_info("2024-02-10 12:00:00")
    assert res["status"] == "ok"
    res["data"]["lunar_time_cn"]

    [When NOT to use]
    Do not use this when you need the reverse direction from lunar date to Gregorian date.
    """
    ctx = load_context()
    try:
        if datetime_str is not None:
            _resolve_datetime(datetime_str)
    except ValueError as exc:
        return invalid_argument_result(str(exc)).to_public_dict()
    result = await run_with_guard(
        primitive_name="get_lunar_info",
        ctx=ctx,
        operation=lambda: _lunar_operation(datetime_str),
    )
    return result.to_public_dict()


async def convert_lunar_to_solar(
    lunar_year: int,
    lunar_month: int,
    lunar_day: int,
    is_leap_month: bool = False,
) -> dict[str, Any]:
    """
    [What it does]
    Converts a lunar date into its Gregorian date.

    [Args]
    lunar_year: Lunar year.
    lunar_month: Lunar month number between 1 and 12.
    lunar_day: Lunar day number between 1 and 31.
    is_leap_month: Whether the lunar month is leap.

    [Output Contract]
    res = await convert_lunar_to_solar(2024, 1, 1)
    assert res["status"] == "ok"
    res["data"]["gregorian_date"]

    [When NOT to use]
    Do not use this when you already have a Gregorian datetime and only need lunar formatting.
    """
    ctx = load_context()
    try:
        parsed_year = int(lunar_year)
        parsed_month = validate_int_range(lunar_month, "lunar_month", 1, 12)
        parsed_day = validate_int_range(lunar_day, "lunar_day", 1, 31)
        if not isinstance(is_leap_month, bool):
            raise ValueError("Parameter is_leap_month must be a boolean")
    except (TypeError, ValueError) as exc:
        return invalid_argument_result(str(exc)).to_public_dict()
    result = await run_with_guard(
        primitive_name="convert_lunar_to_solar",
        ctx=ctx,
        operation=lambda: _lunar_to_solar_operation(
            parsed_year,
            parsed_month,
            parsed_day,
            is_leap_month,
        ),
    )
    return result.to_public_dict()


async def get_sizhu_info(datetime_str: str | None = None) -> dict[str, Any]:
    """
    [What it does]
    Calculates Four Pillars for a Gregorian datetime in China time.

    [Args]
    datetime_str: Datetime string in YYYY-MM-DD HH:MM:SS format. Uses current China time when omitted.

    [Output Contract]
    res = await get_sizhu_info("2024-02-10 12:00:00")
    assert res["status"] == "ok"
    res["data"]["sizhu_cn"]

    [When NOT to use]
    Do not use this when you only need lunar calendar conversion without pillar calculation.
    """
    ctx = load_context()
    try:
        if datetime_str is not None:
            _resolve_datetime(datetime_str)
    except ValueError as exc:
        return invalid_argument_result(str(exc)).to_public_dict()
    result = await run_with_guard(
        primitive_name="get_sizhu_info",
        ctx=ctx,
        operation=lambda: _sizhu_operation(datetime_str),
    )
    return result.to_public_dict()


async def calculate_meihua(
    method: int,
    datetime_str: str | None = None,
) -> dict[str, Any]:
    """
    [What it does]
    Calculates Mei Hua Yi Shu output using time-based mode or local random mode.

    [Args]
    method: Divination mode: 1 for time-based Mei Hua Yi Shu, 3 for random.
    datetime_str: Datetime string in YYYY-MM-DD HH:MM:SS format for time mode. Uses current China time when omitted.

    [Output Contract]
    res = await calculate_meihua(method=1, datetime_str="2024-02-10 12:00:00")
    assert res["status"] == "ok"
    res["data"]["ben_gua"]

    [When NOT to use]
    Do not use this when you only need calendar conversion or Four Pillars without hexagram calculation.
    """
    ctx = load_context()
    try:
        parsed_method = validate_int_range(method, "method", 1, 3)
        if parsed_method not in {1, 3}:
            raise ValueError("Parameter method must be 1 or 3")
        if datetime_str is not None:
            _resolve_datetime(datetime_str)
    except ValueError as exc:
        return invalid_argument_result(str(exc)).to_public_dict()
    result = await run_with_guard(
        primitive_name="calculate_meihua",
        ctx=ctx,
        operation=lambda: _meihua_operation(parsed_method, datetime_str),
    )
    return result.to_public_dict()
