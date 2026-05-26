# Dawn Smart Glasses Page 1 Power Production Intent

版本日期：2026-04-21
状态：生产意图原理图页，可直接转入 KiCad/Altium 做 EVT 板

## 1. 页面边界

Page 1 只负责以下内容：

- USB-C 5 V 输入
- 单节锂电充电与 power-path
- 电池侧 fuel gauge
- 供给 RT685 的 `+1V8_AON`
- 供给 RT685 外部核心电源的 `+1V1_RTCORE`

这一页不再使用旧草案中的 `PCA9420`。原因很直接：系统主充电已经由 `BQ25619` 负责，再级联一个自带 charger 的 PMIC 会让量产时的故障树、热路径和认证边界都变脏。这里改成 `BQ25619 + MAX17048 + 两级离散降压`，是更像真实穿戴式产品量产板的做法。

## 2. 锁定器件

| Ref | 型号 | 用途 |
| --- | --- | --- |
| `J401` | `GCT USB4110-GF-A` | USB Type-C 16-pin receptacle |
| `D401` | `TPD4E05U06DQAR` | USB 口 ESD 保护 |
| `FB401` | `Murata BLM18PG600SN1D` | VBUS EMI 抑制磁珠 |
| `U401` | `BQ25619RTWR` | 单节锂电 charger + power-path |
| `U402` | `MAX17048G+T10` | 电量计 |
| `U403` | `TPS628437DRLR` | `+1V8_AON` buck，固定 1.8 V |
| `U404` | `TPS62841DGR` | `+1V1_RTCORE` buck，RSET 设定 1.1 V |
| `BT401` | `Custom 1S Li-ion pack, 3.7 V nominal, integrated protector + 10 k NTC` | 电池包，按 pack 采购，不用裸电芯 |
| `L401` | `Murata DFE252012P-1R0M=P2` | BQ25619 主电感，1 µH |
| `L402` | `Murata DFE201612E-1R0M=P2` | 1.8 V buck 电感，1 µH |
| `L403` | `Murata DFE201612E-2R2M=P2` | 1.1 V buck 电感，2.2 µH |

## 3. 原理图连接

### 3.1 USB-C 输入

```text
J401 USB4110-GF-A
  A4/A9/B4/B9 VBUS  -> VBUS_RAW_5V
  A5 CC1            -> R401 5.1k -> GND
  B5 CC2            -> R402 5.1k -> GND
  SHIELD            -> C401 1nF/2kV || R403 1M -> CHASSIS_GND

VBUS_RAW_5V
  -> D401 TPD4E05U06DQAR
  -> FB401 600 ohm @100MHz
  -> +VBUS_CHG

+VBUS_CHG
  -> C402 1uF / 25V / X7R
  -> C403 10uF / 25V / X5R
  -> U401.VBUS
  -> U401.VAC
```

设计决定：

- 第一版量产板不做 USB-PD，只做 `5 V sink-only`。
- 因为没有在 Page 1 放 Type-C current advertisement controller，默认输入限流按 `500 mA` 起步，后续由 RT685 通过 I2C 提升，不允许在冷启动阶段直接跑高充电电流。
- `SHIELD` 不直接硬短到数字地，先做 `1 nF + 1 MΩ` 泄放，降低佩戴设备在 ESD 和天线耦合上的风险。

### 3.2 BQ25619 充电与系统电源

```text
U401 BQ25619RTWR
  VBUS, VAC  <- +VBUS_CHG
  PMID       -> internal
  SW         -> L401 1uH -> BAT path
  BTST       -> C404 47nF -> SW
  REGN       -> C405 4.7uF/10V -> GND
  SYS        -> +SYS_MAIN
  BAT        -> +BAT_PACK
  BATSNS     -> +BAT_SENSE_KELVIN
  SDA/SCL    <-> I2C_PWR_1V8
  INT        -> CHG_INT_N
  CE         <- CHG_CE
  QON        <- SW401_PWR
  PSEL       <- boot-safe 500mA strap
  TS         <- 103AT network
```

锁定阻容值：

- `C404` = `47 nF`, `BTST` 对 `SW`
- `C405` = `4.7 uF / 10 V`, `REGN` 去耦
- `C406`, `C407` = `10 uF / 6.3 V`, `SYS` 近端输出电容
- `C408` = `10 uF / 6.3 V`, `BAT` 近端电容
- `R404` = `100 k`, `PSEL -> REGN`
- `R405` = `100 k`, `CE -> GND`
- `R408` = `10 k`, `CHG_INT_N -> +1V8_AON`
- `R409` = `10 k`, `FG_ALRT_N -> +1V8_AON`

`TS` 网络按 `BQ25619` 的 103AT 例图固定：

- `RT1` = `5.36 kΩ / 1%`，`REGN -> TS`
- `RT2` = `31.1 kΩ / 1%`，`TS -> GND`
- `NTC` = pack 内 `10 kΩ @25°C`, `TS -> GND`

设计决定：

- `PSEL` 默认拉高到 `REGN`，让系统在 attach 时默认按 `500 mA` 输入限流进入安全模式。
- RT685 接手后再根据热预算和供电器识别结果写寄存器提高 `IINDPM/ICHG`。
- `CE` 默认低，保证在 MCU 尚未起来时系统也能自主充电。
- `QON` 独立接一个电源键 `SW401`，给出 ship mode 退出和 BATFET 全系统复位能力；不要把确认键复用到 `QON`。
- `I2C_PWR_1V8` 的总线上拉统一放在 Page 2；Page 1 不再重复放 `SCL/SDA` 上拉，只保留各自告警线的上拉。

### 3.3 电池包与 Fuel Gauge

```text
BT401 Protected 1S pack
  PACK+ -> +BAT_PACK
  PACK- -> GND
  NTC   -> BAT_NTC

U402 MAX17048G+T10
  VDD, CELL <- +BAT_PACK
  GND       <- GND
  SDA/SCL   <-> I2C_PWR_1V8
  ALRT      -> FG_ALRT_N
  QSTRT     -> test pad only, DNP default
  C420      = 0.1uF from VDD to GND
```

关键点：

- `MAX17048` 直接挂电池侧，不放在 `SYS` 侧，避免系统掉电时丢失真实 SOC。
- 这版不再加 `PCA9306` 电平转换。`MAX17048` 的 `SCL/SDA/ALRT` 都可以直接拉到 `1.8 V` 逻辑域，减少器件、减少失效点、减少布线长度。

### 3.4 Always-on 1.8 V 电源

```text
U403 TPS628437DRLR
  VIN   <- +SYS_MAIN
  EN    <- +SYS_MAIN through R430 100k
  VSET  -> GND   (fixed 1.8V option)
  SW    -> L402 1uH
  VOUT  -> +1V8_AON
  VOS   -> +1V8_AON sense node

Input/output network
  C430 = 4.7uF / 6.3V / X5R  at VIN
  C431 = 10uF / 6.3V / X5R   at VOUT
  C432 = 100nF / 6.3V / X7R  at VOUT high-frequency decoupling
```

`+1V8_AON` 供给：

- RT685 的 `VDD_AO1V8`
- RT685 的全部 `VDD1V8 / VDDIO`
- `I2C_PWR_1V8`
- `I2C_AON_1V8`
- `SE050`
- `W25Q64JW`
- Page 3/4 传感器与 PDM 麦克风

### 3.5 RT685 外部核心 1.1 V 电源

```text
U404 TPS62841DGR
  VIN   <- +SYS_MAIN
  EN    <- +1V8_AON through R440 100k
  EN    -> C440 100nF -> GND   (about 10 ms delay)
  VSET  -> R441 8.45k / 1% -> GND   (sets 1.1V)
  SW    -> L403 2.2uH
  VOUT  -> +1V1_RTCORE

Input/output network
  C441 = 4.7uF / 6.3V / X5R at VIN
  C442 = 10uF / 6.3V / X5R at VOUT
  C443 = 100nF / 6.3V / X7R at VOUT high-frequency decoupling
```

设计决定：

- 用 `+1V8_AON` 延迟使能 `+1V1_RTCORE`，保证 RT685 的 `1.8 V` 域先建立，再上核心电压。
- `TPS62841` 的 `RSET = 8.45 kΩ` 来自 TI 官方表，对应 `1.1 V`。
- 这一路只给 RT685 `VDDCORE`，不外扩别的负载，避免启动抖动。

## 4. 上电与掉电时序

| 顺序 | 电源/信号 | 目标状态 |
| --- | --- | --- |
| `T0` | 插入 USB-C 或接入电池 | `BQ25619` 建立 `SYS_MAIN` |
| `T1` | `SYS_MAIN` 上升 | `TPS628437` 自动建立 `+1V8_AON` |
| `T2` | `+1V8_AON` 稳定约 10 ms 后 | `TPS62841` 建立 `+1V1_RTCORE` |
| `T3` | Page 2 上的 `RESETN` RC 释放 | RT685 从 FlexSPI NOR 启动 |
| `T4` | RT685 初始化 I2C | 读取 `MAX17048`，配置 `BQ25619`，进入系统策略 |

## 5. PCB 约束

- `BQ25619`、`L401`、`C404`、`C405`、`C406/C407` 必须紧凑成一个热环路，`SW` 铜皮面积只做到必要大小，避免辐射。
- `BATSNS` 必须从电池正端 Kelvin sense 回来，不允许和大电流 `BAT` 铜皮长距离并行后再汇合。
- `MAX17048` 放在电池连接器旁边，不要跨过充电开关电流回路。
- `TPS628437` 和 `TPS62841` 分开摆放，各自输入回路和输出回路独立闭合，避免两个 buck 的热区重叠。
- `+1V1_RTCORE` 走线只去 MCU，不上测试插针、不挂 LED、不挂外设。
- `SHIELD` 区域与天线 keep-out 分开，不允许把 USB 金属壳直接并到射频参考地。

## 6. EVT 必测点

- `TP401` = `VBUS_CHG`
- `TP402` = `SYS_MAIN`
- `TP403` = `BAT_PACK`
- `TP404` = `1V8_AON`
- `TP405` = `1V1_RTCORE`
- `TP406` = `CHG_INT_N`
- `TP407` = `FG_ALRT_N`

## 7. 量产前必须关口

- 验证 `PSEL -> REGN` 默认上拉在所有 attach 场景下都能稳定进 `500 mA` 安全态。
- 在最高环境温度、佩戴状态、充电状态同时存在时做 `BQ25619` 热像。
- 校准 `MAX17048` 学习参数与电池 pack 的真实容量，不允许沿用默认 profile 直接量产。
- 电池必须按 pack 采购并带保护，不接受“裸电芯 + 主板兜底”做法。

## 8. 设计依据

- TI `BQ25619` datasheet, revised February 2025
- ADI `MAX17048/MAX17049` datasheet
- TI `TPS62843` datasheet
- TI `TPS62840/TPS62841/TPS62842` datasheet
