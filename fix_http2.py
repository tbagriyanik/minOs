import sys

path = 'f:\\minos\\src\\drivers\\http.rs'
with open(path, 'r', encoding='utf-8') as f:
    content = f.read()

# Fix warnings: remove unnecessary mut
content = content.replace(
    'let (status, hlen, mut total) = result.ok_or(socket::E_INVAL)??;',
    'let (status, hlen, total) = result.ok_or(socket::E_INVAL)??;'
)

content = content.replace(
    '        let mut result: Option<Result<(u16, usize, usize), i32>> = None;',
    '        let result: Option<Result<(u16, usize, usize), i32>> = None;'
)

with open(path, 'w', encoding='utf-8') as f:
    f.write(content)

print('Done')
