---
id: M-069
scope: project
category: fact
title: cargo test 报 unrecognized argument/invalid value:检查ArgParser解析规则
description: 处理 cargo test run build 报 unrecognized argument/invalid value + Usage/help 时必读:检查是否把--、[OPTIONS]等参数格式搞错;cargo.exe的argparser严格区分长选项(--xyz)和短形式(无连字符),对带::符号的值总是视为新词而非值
status: candidate
created: 2026-08-13
updated: 2026-08-13
source: memory-manager
---

[fp:bash|error: unexpected argument 'test_record::' found]

**适用场景**: cargo test/build 时报 unrecognized_argument、invalid value/ "unexpected argument xxx" + Usage/help。ArgParser将带特殊符号的arg视为多个tokens而非字面值拼接结果;任何::或--都将触发解析失败。Rustc/cargo命令行参数不自动做字符串拼接,必须严格按照长选项(--xyz)和短形式(-x无连符),用等号分隔值时格式为xxx=yyy或—no-arg=value，不可写-x:val(会被看成两个词)。

**操作步骤**:
1. 立即检查测试名构造是否含特殊符号::、--、=:ArgParser会将其解析成多个tokens;cargo test运行参数必须严格符合长选项(--xyz)、短形式(-x无连符)或等值写法(xxx=yyy/—no-arg=value)。

2. Cargo的ArgParser将"..."视为词边界:任何特殊符号都是分隔符。不要依赖shell做字符串拼接后再传给cargo(引号未转义时cargo看到的是变量名而非字面值),应先用$(...)或单引号包裹传参，或将需带符号的值拆成多个参数段传递。

**已知触发条件**: cargo test run/build 报"unrecognized_argument", "invalid value 'xxx'",或伴随Usage/help信息的错误;测试方法构造时用错格式导致cargo误判为多个token而非单个字符串值
