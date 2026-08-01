with open('src/types.rs', 'r') as f:
    content = f.read()

old = '''#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RelationKind {
    CapabilityDelegation = 0,
    ContractDependency = 1,
    EffectPropagation = 2,
}'''

new = '''#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkType {
    DependsOn = 0,
    Owns = 1,
    References = 2,
}'''

content = content.replace(old, new)

# Update LinkEdge to use LinkType
content = content.replace('pub relation: RelationKind,', 'pub link_type: LinkType,')

with open('src/types.rs', 'w') as f:
    f.write(content)

print('Done: types.rs updated')
