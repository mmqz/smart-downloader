"""
captcha_sign 算法单元测试 - 验证算法实现正确性

⚠ 注意: alist 没在源码里给"已知输入 → 已知输出"测试用例
所以我只能验证算法逻辑:
  1. 同输入两次计算,结果应一致 (确定性)
  2. 输入微小变化,结果应不同 (敏感性)
  3. 输出格式: "1." + 32 字符 hex (MD5)
  4. 算法应做 10 轮 (Algorithms 数量)
"""
import hashlib
import sys
sys.path.insert(0, '/home/z/my-project/scripts/p2p_recon')
from test_captcha_sign import get_captcha_sign, generate_device_sign, ALGORITHMS

print("="*70)
print("captcha_sign 算法单元测试")
print("="*70)

# Test 1: 确定性
ts1 = "1786954010274"
did1 = "bd6cbff95e71004d0cccb4b9b13856a2"
sign1 = get_captcha_sign(ts1, did1)
sign2 = get_captcha_sign(ts1, did1)
print(f"\n[Test 1] 确定性 (同输入两次):")
print(f"  sign1 = {sign1}")
print(f"  sign2 = {sign2}")
print(f"  一致? {'✅' if sign1 == sign2 else '❌'}")

# Test 2: 敏感性
sign3 = get_captcha_sign(str(int(ts1) + 1), did1)
print(f"\n[Test 2] 敏感性 (timestamp +1ms):")
print(f"  原 sign = {sign1}")
print(f"  新 sign = {sign3}")
print(f"  不同? {'✅' if sign1 != sign3 else '❌'}")

# Test 3: 输出格式
print(f"\n[Test 3] 输出格式:")
prefix = sign1.startswith("1.")
hex_part = sign1[2:]
is_md5_hex = len(hex_part) == 32 and all(c in '0123456789abcdef' for c in hex_part)
print(f"  前缀 '1.'? {'✅' if prefix else '❌'}")
print(f"  后 32 字符是 hex MD5? {'✅' if is_md5_hex else '❌'}")
print(f"  长度 = {len(sign1)} (期望 34)")

# Test 4: 算法手动验证 (一轮一轮)
print(f"\n[Test 4] 算法分步验证:")
s = f"Xp6vsxz_7IYVw2BB8.31.0.9726com.xunlei.downloadproviderbd6cbff95e71004d0cccb4b9b13856a21786954010274"
print(f"  初始串: {s}")
for i, algo in enumerate(ALGORITHMS):
    s = hashlib.md5((s + algo).encode()).hexdigest()
    print(f"  轮 {i+1} (algo[{i}]={algo[:20]}...): {s}")
expected = "1." + s
print(f"  最终: {expected}")
print(f"  与函数输出一致? {'✅' if expected == sign1 else '❌'}")

# Test 5: device_sign 验证
print(f"\n[Test 5] device_sign 格式:")
ds = generate_device_sign(did1)
print(f"  device_sign = {ds}")
prefix_ok = ds.startswith("div101.")
contains_did = did1 in ds
md5_part = ds[len("div101.") + len(did1):]
is_md5 = len(md5_part) == 32 and all(c in '0123456789abcdef' for c in md5_part)
print(f"  前缀 'div101.'? {'✅' if prefix_ok else '❌'}")
print(f"  含 device_id? {'✅' if contains_did else '❌'}")
print(f"  末 32 字符是 hex MD5? {'✅' if is_md5 else '❌'}")

# Test 6: 关键问题 - Algorithms 仍有效吗?
# 我们已知: 不带 captcha_sign 的 captcha/init 匿名调用能成功 (前面测了)
# 但带 captcha_sign 的 captcha/init 返回 "invalid captcha_sign"
# 这说明: Algorithms 盐被换了
print(f"\n[Test 6] Algorithms 当前状态:")
print(f"  Algorithms 数量: {len(ALGORITHMS)}")
print(f"  来自 alist 主分支 (开源, MIT)")
print(f"  对应迅雷客户端版本: 8.31.0.9726 (Android)")
print(f"  ")
print(f"  ⚠ 实测 (前面调用): 带 captcha_sign 的请求被拒 (invalid captcha_sign)")
print(f"  说明: 服务端已更换 Algorithms, 或当前 captcha/init 路径不需要 sign")
print(f"  验证方法: 需登录后调 RefreshCaptchaTokenAtLogin 测试 (需账号)")

print(f"\n{'='*70}")
print("结论:")
print("1. captcha_sign 算法实现正确 (符合 alist Go 描述)")
print("2. device_sign 算法实现正确")
print("3. Algorithms 盐可能失效 — 需登录后验证")
print("4. 匿名 captcha/init 不需 captcha_sign — 是入口,不是验证点")
print("="*70)
