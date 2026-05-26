# Dawn Smart Glasses Page 2 RT685 Production Intent

版本日期：2026-04-21
状态：生产意图原理图页，可直接转入 KiCad/Altium 做 EVT 板

## 1. 页面边界

Page 2 只冻结 RT685 always-on 控制域：

- `MIMXRT685SFVKB` 本体
- 外部 `1.1 V` 核心供电接法
- `1.8 V` IO/AON 供电与去耦
- 启动脚位
- FlexSPI-A 启动 Flash
- 调试口、串口 ISP、物理确认键
- 安全元件 `SE050`

这一页故意不把 AR1 模块接口画死到每一个 pin。原因不是偷懒，而是 Page 5 仍要跟 AR1 模块/参考设计约束对齐。Page 2 先把 RT685 自身做成可投板状态，才是正确工程顺序。

## 2. 锁定器件

| Ref | 型号 | 用途 |
| --- | --- | --- |
| `U200` | `MIMXRT685SFVKB` | 主 always-on MCU，VFBGA176 |
| `U220` | `SE050E2HQ1/Z01Z3Z` | 设备身份、签名、密钥存储 |
| `U230` | `Winbond W25Q64JWSSIQ` | 1.8 V Quad SPI NOR Boot Flash |
| `Y200` | `NDK NX2016SA-24.000M-STD-CSR-1` | 24 MHz 主晶振 |
| `J201` | `Tag-Connect TC2030-IDC-NL` | SWD 调试接口 |
| `SW201` | `TL3342F160QG/TR` | `CONFIRM` 物理确认键 |
| `SW202` | `TL3342F160QG/TR` | `CANCEL` 物理取消键 |
| `SW203` | `TL3342F160QG/TR` | `FORCE_ISP` 生产救援键，可选 |
| `SW204` | `TL3342F160QG/TR` | `RESET` 复位键 |

## 3. 供电与复位

### 3.1 供电策略

RT685 这版全部 IO 都锁定到 `1.8 V`：

- `VDD_AO1V8` <- `+1V8_AON`
- `VDD1V8` <- `+1V8_AON`
- `VDD1V8_1` <- `+1V8_AON`
- 所有 `VDDIOx` <- `+1V8_AON`
- 所有 `VDDCORE` <- `+1V1_RTCORE`

这样做的原因：

- 传感器、Flash、安全元件和 BQ25619 控制面本来就都能跑 `1.8 V`
- 省掉 Page 2 上的任何额外电平转换
- 简化 EMC 和低功耗分析

### 3.2 去耦网络

推荐去耦组：

- `C201-C208` = `100 nF / 6.3 V / X7R / 0402`，每个 `1.8 V` 供电脚组就近放置
- `C209`, `C210` = `4.7 uF / 6.3 V / X5R / 0402`，每个 `1.8 V` bank 一个 bulk
- `C211-C214` = `4.7 uF / 6.3 V / X5R / 0402`，围绕 `VDDCORE` 分散放置
- `C215`, `C216` = `100 nF / 6.3 V / X7R / 0402`，补在核心域高频位置

### 3.3 外部核心电源模式

RT685 使用外部核心电源，因此：

```text
LDO_ENABLE -> R201 10k -> GND
```

`RESETN` 网络固定为：

```text
RESETN -> R202 100k -> +1V8_AON
RESETN -> C217 100nF -> GND
RESETN -> SW204 reset tact switch -> GND
RESETN -> J201 nRESET
```

说明：

- `R202 100 kΩ` 是按 NXP 数据手册建议保留的上拉。
- 这里用 `100 k + 100 nF` 让 `RESETN` 在 `+1V1_RTCORE` 建立之后再释放，足够覆盖 Page 1 中 `TPS62841` 的软启动。
- 如果 EVT 冷启动测试显示边缘不够稳，再把 `RESETN` 改成独立 supervisor，不要一开始就把页复杂度做爆。

## 4. Pin/Mux 冻结

| 功能 | RT685 复用 | 说明 |
| --- | --- | --- |
| `UART0_TX` | `PIO0_1 / FC0_TXD` | 串口 ISP 与产测日志 |
| `UART0_RX` | `PIO0_2 / FC0_RXD` | 串口 ISP 与产测日志 |
| `I2C_PWR_SCL` | `PIO0_29 / FC4_TXD_SCL` | 到 `BQ25619` 与 `MAX17048` |
| `I2C_PWR_SDA` | `PIO0_30 / FC4_RXD_SDA` | 到 `BQ25619` 与 `MAX17048` |
| `I2C_AON_SDA` | `PIO1_6 / FC5_CTS_SDA` | 到 `SE050` 与 Page 3 低速传感器 |
| `I2C_AON_SCL` | `PIO1_7 / FC5_RTS_SCL` | 到 `SE050` 与 Page 3 低速传感器 |
| `GPIO_CONFIRM_N` | `PIO0_12` | 物理确认键 |
| `GPIO_CANCEL_N` | `PIO0_13` | 物理取消键 |
| `CHG_INT_N` | `PIO0_14` | `BQ25619` interrupt 输入 |
| `FG_ALRT_N` | `PIO0_15` | `MAX17048` alert 输入 |
| `SE_RST_N` | `PIO0_16` | `SE050` 复位/使能控制 |
| `WAKE_HOST_N` | `PIO0_17` | 到 Page 5 主机唤醒握手 |
| `ISP0` | `PIO1_15` | 默认上拉 |
| `ISP1` | `PIO1_16` | 默认上拉 |
| `ISP2` | `PIO1_17` | 默认下拉 |
| `FLEXSPI_A_SCLK` | `PIO1_18` | Boot NOR 时钟 |
| `FLEXSPI_A_SS0_N` | `PIO1_19` | Boot NOR 片选 |
| `FLEXSPI_A_DATA0` | `PIO1_20` | QSPI IO0 |
| `FLEXSPI_A_DATA1` | `PIO1_21` | QSPI IO1 |
| `FLEXSPI_A_DATA2` | `PIO1_22` | QSPI IO2 / WP# |
| `FLEXSPI_A_DATA3` | `PIO1_23` | QSPI IO3 / HOLD# |
| `PDM_CLK01` | `PIO3_0` | 前端双麦时钟 |
| `PDM_CLK23` | `PIO3_1` | 镜腿双麦时钟 |
| `PDM_DATA01` | `PIO3_4` | 前端双麦数据 |
| `PDM_DATA23` | `PIO3_5` | 镜腿双麦数据 |

保留但不使用：

- `PMIC_I2C_SDA`
- `PMIC_I2C_SCL`
- `PMIC_IRQ_N`

其中 `PMIC_IRQ_N` 仍保留一个上拉：

```text
PMIC_IRQ_N -> R203 10k -> +1V8_AON
```

## 5. 24 MHz 时钟

为保证串口 ISP 之外还保有 USB/时钟 bring-up 余量，Page 2 固定 24 MHz 晶振：

```text
Y200 24.000 MHz
  XTAL_IN  <-> Y200 <-> XTAL_OUT
  C221 = 12pF / NP0 -> GND
  C222 = 12pF / NP0 -> GND
  R220 = 33 ohm series on XTAL_OUT, DNI by default but keep footprint
```

说明：

- 这颗晶振不是“装饰件”，它是给量产阶段的 USB/时钟 bring-up 留的逃生口。
- `12 pF` 是基于 `8 pF` 负载晶振和约 `2 pF` 板级寄生的首版值，EVT 后按启动裕量和 ppm 再修。

## 6. Boot Flash 与启动脚

### 6.1 FlexSPI-A NOR

```text
U230 W25Q64JWSSIQ
  VCC      <- +1V8_AON
  GND      <- GND
  /CS      <- FLEXSPI_A_SS0_N through R230 22 ohm
  CLK      <- FLEXSPI_A_SCLK through R231 22 ohm
  IO0      <-> FLEXSPI_A_DATA0 through R232 22 ohm
  IO1      <-> FLEXSPI_A_DATA1 through R233 22 ohm
  IO2/WP#  <-> FLEXSPI_A_DATA2 through R234 22 ohm
  IO3/HOLD#<-> FLEXSPI_A_DATA3 through R235 22 ohm
  C230     = 100nF -> GND
  C231     = 1uF -> GND
```

说明：

- 首版就给 `22 Ω` 串阻位，避免 FlexSPI 首板 bring-up 时只能靠割线救场。
- `W25Q64JW` 容量足够承载 RT685 always-on 固件、声学模型和恢复镜像。

### 6.2 ISP Strap

默认启动方式固定成 `FlexSPI Port A boot`：

```text
PIO1_15 ISP0 -> R240 47k -> +1V8_AON
PIO1_16 ISP1 -> R241 47k -> +1V8_AON
PIO1_17 ISP2 -> R242 47k -> GND
```

同时给出三组产测覆点：

- `TP220` -> `ISP0`
- `TP221` -> `ISP1`
- `TP222` -> `ISP2`

如果要强制进 ISP：

- `SW203` 把 `ISP0` 拉低到 `GND`
- 保留 `UART0_TX/RX` 到调试口

## 7. I2C 总线

### 7.1 Power Management Bus

```text
I2C_PWR_1V8
  SDA = PIO0_30
  SCL = PIO0_29
  R250 = 4.7k -> +1V8_AON
  R251 = 4.7k -> +1V8_AON
  devices:
    U401 BQ25619
    U402 MAX17048
```

### 7.2 Always-on Peripheral Bus

```text
I2C_AON_1V8
  SDA = PIO1_6
  SCL = PIO1_7
  R252 = 2.2k -> +1V8_AON
  R253 = 2.2k -> +1V8_AON
  devices:
    U220 SE050
    Page 3 TMF8828
    Page 3 TMD3725
```

分两条总线而不是混在一条上，原因是：

- 电源管理总线必须在最小系统里就能跑起来
- Page 3 的传感器、线长和热插拔扰动不应该影响 charger/gauge 读数

## 8. 安全元件与物理确认

```text
U220 SE050E2HQ1/Z01Z3Z
  VCC      <- +1V8_AON
  GND      <- GND
  SDA/SCL  <-> I2C_AON_1V8
  C240     = 100nF -> GND
  C241     = 1uF -> GND
  VEN/RST  <- PIO0_16 (SE_RST_N), default pull-up 100k to +1V8_AON
```

按键：

```text
SW201 CONFIRM -> GPIO_CONFIRM_N -> GND when pressed
SW202 CANCEL  -> GPIO_CANCEL_N  -> GND when pressed

R260 = 47k -> +1V8_AON on CONFIRM line
R261 = 47k -> +1V8_AON on CANCEL line
C260 = 1nF -> GND on CONFIRM line
C261 = 1nF -> GND on CANCEL line
```

这里的职责是明确的：

- RT685 负责物理事件采样
- `SE050` 负责设备证书和签名
- Dawn 的审批/确认模型跑在上层系统，但最终确认动作由这条硬件链闭环
- `CHG_INT_N` 和 `FG_ALRT_N` 固定接到 `PIO0_14/PIO0_15`，这样 Page 1 的电源事件可以在最小系统阶段就被 RT685 捕获

## 9. 调试与产测

### 9.1 SWD

`J201 Tag-Connect TC2030-IDC-NL`

- `VTREF` = `+1V8_AON`
- `SWDIO`
- `SWCLK`
- `nRESET`
- `GND`
- `UART0_TX` 复用到产测治具

### 9.2 最小救援能力

必须保留：

- `UART0_TX/RX`
- `RESETN`
- `ISP0/1/2`

这不是可选项。RT600 的量产调试如果没有 Serial ISP 逃生口，后续你会浪费大量时间在“坏板是硬件问题还是镜像问题”的无效排查上。

## 10. 版图约束

- `U230` 必须贴在 `U200` 的 `FlexSPI-A` 引脚扇出一侧，`CLK` 线最短，数据线等长控制在同一组内。
- `Y200` 贴近 XTAL 引脚，晶振地环独立回流，不允许跨分割。
- `+1V1_RTCORE` 只做短粗铜皮，不穿长测试走线。
- `SE050` 与按键、调试口放在 RT685 一侧，避免把安全域跨过麦克风/射频区域。
- `I2C_PWR_1V8` 与 `I2C_AON_1V8` 分开布线，不共享长 stub。

## 11. Bring-up 顺序

1. 仅装 Page 1 + Page 2，先不上 Page 3/4/5。
2. 验证 `1V8_AON`、`1V1_RTCORE`、`RESETN` 时序。
3. 通过 `UART0` + `ISP` 拉起 RT685 ROM ISP。
4. 烧入最小 FlexSPI NOR 镜像，验证冷启动。
5. 读取 `BQ25619` 和 `MAX17048`，确认两条 I2C 总线都正常。
6. 再接入 `SE050`，最后才接 Page 3 传感器和 PDM 麦阵。

## 12. 设计依据

- NXP `RT600` datasheet, rev 2.5, 19 June 2025
- NXP `i.MX RT600 Hardware Design – Part 2 of 3 – Flash Memory, Boot and Debug`
- NXP `EdgeLock SE050` product family leaflet
