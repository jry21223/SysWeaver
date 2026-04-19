/// CRITICAL 级别：不可逆破坏，直接拒绝，不给确认机会
pub const CRITICAL_PATTERNS: &[&str] = &[
    r"rm\s+-[rf]{1,2}f?\s+/[^/]",          // rm -rf /xxx (根目录下)
    r"rm\s+-[rf]{1,2}f?\s+/$",             // rm -rf /
    r"mkfs\.",                             // 格式化磁盘
    r"dd\s+.*of=/dev/[sh]d[a-z]",          // dd 直写磁盘
    r">\s*/dev/[sh]d[a-z]",                // 重定向写磁盘设备
    r":\(\)\{.*:\|:&\};:",                 // Fork bomb 变体
    r":(?:\(\)|\{)\s*\{[^}]*:\s*&",        // Fork bomb
    r"chmod\s+-R\s+[0-7]*7[0-7]*\s+/\s*$", // chmod 777 /
    r"shred\s+.*-[uzn]",                   // 安全擦除磁盘
];

/// HIGH 级别：不易恢复，必须用户确认
pub const HIGH_PATTERNS: &[&str] = &[
    r"userdel\s+",                     // 删除用户
    r"passwd\s+root",                  // 修改 root 密码
    r"systemctl\s+stop\s+sshd?(\s|$)", // 停止 SSH 服务
    r"systemctl\s+disable\s+sshd?",    // 禁用 SSH 服务
    r"iptables\s+-F",                  // 清空防火墙规则
    r"iptables\s+-X",                  // 删除所有链
    r"ufw\s+--force\s+reset",          // 重置防火墙
    r"crontab\s+-r",                   // 删除所有定时任务
    r"visudo",                         // 修改 sudo 规则
    r"rm\s+.*\.(key|pem|crt|cert)",    // 删除证书/密钥文件
    r"chmod\s+-R\s+[0-7]+\s+/etc",     // 递归修改 /etc 权限
    r"chattr\s+\+i\s+/",               // 给根目录文件加锁
    r"echo\s+.*>+\s*/etc/shadow",      // 覆写 shadow 文件
    r"truncate\s+.*-s\s+0.*/(passwd|shadow|sudoers)",
];

/// MEDIUM 级别：可逆但有影响，可配置是否自动确认
pub const MEDIUM_PATTERNS: &[&str] = &[
    r"useradd\s+",                         // 创建用户
    r"usermod\s+",                         // 修改用户
    r"groupadd\s+",                        // 创建用户组
    r"systemctl\s+(restart|stop)\s+\w+",   // 重启/停止服务
    r"chmod\s+[0-7]{3,4}\s+",              // 修改文件权限
    r"chown\s+",                           // 修改文件归属
    r"apt[\s-]+(remove|purge|autoremove)", // 卸载软件
    r"yum\s+(remove|erase)",               // 卸载软件 (RHEL)
    r"dnf\s+(remove|erase)",               // 卸载软件 (Fedora)
    r"pip\s+uninstall",                    // 卸载 Python 包
    r"rm\s+-rf?\s+/var/",                  // 删除 /var 下的内容
    r"rm\s+-rf?\s+/home/",                 // 删除 /home 下内容
    r">\s*/etc/",                          // 覆写 /etc 下配置文件
];

/// 读取操作中需要注意的敏感文件（不阻止，但在反馈中提示）
// Reserved for future sensitive-read warning feature
#[allow(dead_code)]
pub const SENSITIVE_READ_PATHS: &[&str] = &[
    "/etc/shadow",
    "/etc/passwd",
    "/etc/sudoers",
    "id_rsa",
    ".ssh/",
    ".env",
    "*.key",
    "*.pem",
];
