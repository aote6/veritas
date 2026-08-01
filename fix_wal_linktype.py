with open('src/engine.rs', 'r') as f:
    content = f.read()

# Fix decode (WAL -> enum)
old_decode = '''                    let relation = match link_type {
                        0 => LinkType::DependsOn,
                        1 => LinkType::DependsOn,
                        2 => LinkType::References,
                        _ => continue,
                    };'''

new_decode = '''                    let relation = match link_type {
                        0 => LinkType::DependsOn,
                        1 => LinkType::Owns,
                        2 => LinkType::References,
                        _ => continue,
                    };'''

content = content.replace(old_decode, new_decode)

# Fix encode (enum -> WAL)
old_encode = '''                link_type: match edge.link_type {
                    LinkType::DependsOn => 0,
                    LinkType::DependsOn => 1,
                    LinkType::References => 2,
                },'''

new_encode = '''                link_type: match edge.link_type {
                    LinkType::DependsOn => 0,
                    LinkType::Owns => 1,
                    LinkType::References => 2,
                },'''

content = content.replace(old_encode, new_encode)

with open('src/engine.rs', 'w') as f:
    f.write(content)

print('Done')
