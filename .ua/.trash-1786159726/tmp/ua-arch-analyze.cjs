#!/usr/bin/env node
'use strict';

const fs = require('fs');
const path = require('path');

function fail(msg) {
  console.error(msg);
  process.exit(1);
}

function loadInput(p) {
  let raw;
  try {
    raw = fs.readFileSync(p, 'utf8');
  } catch (e) {
    fail(`Failed to read input: ${e.message}`);
  }
  try {
    return JSON.parse(raw);
  } catch (e) {
    fail(`Failed to parse input JSON: ${e.message}`);
  }
}

const DIR_PATTERNS = [
  [['routes', 'api', 'controllers', 'endpoints', 'handlers', 'routers', 'controller', 'serializers', 'blueprints'], 'api'],
  [['services', 'core', 'lib', 'domain', 'logic', 'internal', 'signals', 'composables', 'mailers', 'jobs', 'channels'], 'service'],
  [['models', 'db', 'data', 'persistence', 'repository', 'entities', 'migrations', 'entity', 'sql', 'database', 'schema'], 'data'],
  [['components', 'views', 'pages', 'ui', 'layouts', 'screens'], 'ui'],
  [['middleware', 'plugins', 'interceptors', 'guards'], 'middleware'],
  [['utils', 'helpers', 'common', 'shared', 'tools', 'templatetags', 'pkg'], 'utility'],
  [['config', 'constants', 'env', 'settings', 'management', 'commands'], 'config'],
  [['__tests__', 'test', 'tests', 'spec', 'specs'], 'test'],
  [['types', 'interfaces', 'schemas', 'contracts', 'dtos', 'dto', 'request', 'response'], 'types'],
  [['hooks'], 'hooks'],
  [['store', 'state', 'reducers', 'actions', 'slices'], 'state'],
  [['assets', 'static', 'public'], 'assets'],
  [['cmd', 'bin'], 'entry'],
  [['docs', 'documentation', 'wiki'], 'documentation'],
  [['deploy', 'deployment', 'infra', 'infrastructure', 'k8s', 'kubernetes', 'helm', 'charts', 'terraform', 'tf', 'docker'], 'infrastructure'],
  [['.github', '.gitlab', '.circleci'], 'ci-cd'],
];

function matchDirPattern(name) {
  const lower = name.toLowerCase();
  for (const [names, label] of DIR_PATTERNS) {
    if (names.includes(lower)) return label;
  }
  return null;
}

function fileLevelPattern(filePath, baseName) {
  const fp = filePath.replace(/\\/g, '/');
  const bn = baseName;
  if (/\.(test|spec)\./i.test(bn) || /^test_.*\.py$/i.test(bn) || /_test\.go$/i.test(bn) ||
      /Test\.java$/i.test(bn) || /_spec\.rb$/i.test(bn) || /Test\.php$/i.test(bn) || /Tests\.cs$/i.test(bn) ||
      /\.selfcheck\.(ts|mjs|js)$/i.test(bn)) {
    return 'test';
  }
  if (/\.d\.ts$/i.test(bn)) return 'types';
  if (/^(Dockerfile|docker-compose)/i.test(bn)) return 'infrastructure';
  if (/\.tf(vars)?$/i.test(bn)) return 'infrastructure';
  if (bn === 'Makefile') return 'infrastructure';
  if (/\.(md|rst)$/i.test(bn)) return 'documentation';
  if (/\.sql$/i.test(bn)) return 'data';
  if (/\.(graphql|gql|proto)$/i.test(bn)) return 'types';
  if (/\.github\/workflows\//.test(fp) || bn === '.gitlab-ci.yml' || bn === 'Jenkinsfile') return 'ci-cd';
  if (['Cargo.toml', 'go.mod', 'Gemfile', 'pom.xml', 'build.gradle', 'composer.json', 'package.json'].includes(bn)) {
    return 'config';
  }
  if (bn === 'main.rs' || bn === 'lib.rs') {
    if (/\/src\/[^/]+$/.test(fp) || /^src\/[^/]+$/.test(fp) || /src-tauri\/src\/[^/]+$/.test(fp)) return 'entry';
  }
  if (bn === 'main.go' && /\/cmd\//.test(fp)) return 'entry';
  if (['index.ts', 'index.js', '__init__.py'].includes(bn)) return 'entry';
  if (bn === 'manage.py' && !fp.includes('/')) return 'entry';
  if (bn === 'wsgi.py' || bn === 'asgi.py') return 'config';
  if (bn === 'Application.java' || bn === 'Program.cs' || bn === 'config.ru') return 'entry';
  return null;
}

function commonPrefix(paths) {
  if (!paths.length) return '';
  const split = paths.map((p) => p.replace(/\\/g, '/').split('/').filter(Boolean));
  const minLen = Math.min(...split.map((s) => s.length));
  let i = 0;
  while (i < minLen) {
    const seg = split[0][i];
    if (split.every((s) => s[i] === seg)) i++;
    else break;
  }
  // Only treat as common directory prefix if all paths still have a segment after it
  // and the prefix ends at a directory boundary used by all.
  if (i === 0) return '';
  // If every path is exactly the prefix (single file in root of common), keep it
  const allHaveMore = split.every((s) => s.length > i);
  if (!allHaveMore) {
    // reduce until grouping is useful
    while (i > 0 && !split.every((s) => s.length > i)) i--;
  }
  return i > 0 ? split[0].slice(0, i).join('/') + '/' : '';
}

function groupKeyForPath(filePath, prefix) {
  const fp = filePath.replace(/\\/g, '/');
  let rest = fp;
  if (prefix && fp.startsWith(prefix)) rest = fp.slice(prefix.length);
  const parts = rest.split('/').filter(Boolean);
  if (parts.length <= 1) {
    // flat or root file
    return 'root';
  }
  return parts[0];
}

function main() {
  const inputPath = process.argv[2];
  const outputPath = process.argv[3];
  if (!inputPath || !outputPath) fail('Usage: ua-arch-analyze.js <input.json> <output.json>');

  const input = loadInput(inputPath);
  const fileNodes = input.fileNodes || [];
  const importEdges = input.importEdges || [];
  const allEdges = input.allEdges || [];
  if (!fileNodes.length) fail('No fileNodes in input');

  const paths = fileNodes.map((n) => n.filePath || '').filter(Boolean);
  const prefix = commonPrefix(paths);

  // A. Directory grouping — for mixed monorepo (src + src-tauri + root), common prefix may be empty.
  // Prefer first segment; for nested src-tauri/src treat as src-tauri.
  const directoryGroups = {};
  for (const n of fileNodes) {
    const fp = (n.filePath || '').replace(/\\/g, '/');
    let key;
    if (!fp) key = 'unknown';
    else if (!fp.includes('/')) key = 'root';
    else {
      const parts = fp.split('/');
      // Use top-level segment; for src and src-tauri keep as group
      key = parts[0];
      // Subgroup signal: src/components etc. stored separately? Spec says first after common prefix.
      if (prefix && fp.startsWith(prefix)) {
        key = groupKeyForPath(fp, prefix);
      }
    }
    if (!directoryGroups[key]) directoryGroups[key] = [];
    directoryGroups[key].push(n.id);
  }

  // Also compute finer groups for pattern matching (second level for src/* and src-tauri/*)
  const fineGroups = {};
  for (const n of fileNodes) {
    const fp = (n.filePath || '').replace(/\\/g, '/');
    const parts = fp.split('/').filter(Boolean);
    let key = 'root';
    if (parts.length === 0) key = 'unknown';
    else if (parts.length === 1) key = 'root';
    else if (parts[0] === 'src' || parts[0] === 'src-tauri') {
      key = parts.length >= 2 ? `${parts[0]}/${parts[1]}` : parts[0];
    } else {
      key = parts[0];
    }
    if (!fineGroups[key]) fineGroups[key] = [];
    fineGroups[key].push(n.id);
  }

  // B. Node type grouping
  const nodeTypeGroups = {};
  for (const n of fileNodes) {
    const t = n.type || 'file';
    if (!nodeTypeGroups[t]) nodeTypeGroups[t] = [];
    nodeTypeGroups[t].push(n.id);
  }

  // C. Import adjacency
  const idSet = new Set(fileNodes.map((n) => n.id));
  const fanOut = {};
  const fanIn = {};
  for (const id of idSet) {
    fanOut[id] = 0;
    fanIn[id] = 0;
  }
  const adj = {};
  for (const e of importEdges) {
    if (!idSet.has(e.source) || !idSet.has(e.target)) continue;
    fanOut[e.source] = (fanOut[e.source] || 0) + 1;
    fanIn[e.target] = (fanIn[e.target] || 0) + 1;
    if (!adj[e.source]) adj[e.source] = [];
    adj[e.source].push(e.target);
  }

  const idToGroup = {};
  for (const [g, ids] of Object.entries(directoryGroups)) {
    for (const id of ids) idToGroup[id] = g;
  }

  // E. Inter-group import frequency
  const interMap = {};
  for (const e of importEdges) {
    if (!idSet.has(e.source) || !idSet.has(e.target)) continue;
    const from = idToGroup[e.source];
    const to = idToGroup[e.target];
    if (!from || !to || from === to) continue;
    const k = `${from}\0${to}`;
    interMap[k] = (interMap[k] || 0) + 1;
  }
  const interGroupImports = Object.entries(interMap)
    .map(([k, count]) => {
      const [from, to] = k.split('\0');
      return { from, to, count };
    })
    .sort((a, b) => b.count - a.count);

  // F. Intra-group density
  const intraGroupDensity = {};
  for (const g of Object.keys(directoryGroups)) {
    let internalEdges = 0;
    let totalEdges = 0;
    for (const e of importEdges) {
      if (!idSet.has(e.source) || !idSet.has(e.target)) continue;
      const fs_ = idToGroup[e.source];
      const ft = idToGroup[e.target];
      if (fs_ !== g && ft !== g) continue;
      totalEdges++;
      if (fs_ === g && ft === g) internalEdges++;
    }
    intraGroupDensity[g] = {
      internalEdges,
      totalEdges,
      density: totalEdges ? internalEdges / totalEdges : 0,
    };
  }

  // D. Cross-category
  const crossMap = {};
  for (const e of allEdges) {
    const src = fileNodes.find((n) => n.id === e.source);
    const tgt = fileNodes.find((n) => n.id === e.target);
    if (!src || !tgt) continue;
    if (src.type === tgt.type && src.type === 'file' && e.type === 'imports') continue;
    const key = `${src.type}\0${tgt.type}\0${e.type || 'unknown'}`;
    crossMap[key] = (crossMap[key] || 0) + 1;
  }
  // Also count all type pairs including imports for completeness per spec
  const crossAll = {};
  for (const e of allEdges) {
    const src = fileNodes.find((n) => n.id === e.source);
    const tgt = fileNodes.find((n) => n.id === e.target);
    if (!src || !tgt) continue;
    const key = `${src.type}\0${tgt.type}\0${e.type || 'unknown'}`;
    crossAll[key] = (crossAll[key] || 0) + 1;
  }
  const crossCategoryEdges = Object.entries(crossAll)
    .map(([k, count]) => {
      const [fromType, toType, edgeType] = k.split('\0');
      return { fromType, toType, edgeType, count };
    })
    .sort((a, b) => b.count - a.count);

  // G. Pattern matching
  const patternMatches = {};
  for (const g of Object.keys(directoryGroups)) {
    const m = matchDirPattern(g);
    if (m) patternMatches[g] = m;
    else {
      // check fine name
      const leaf = g.includes('/') ? g.split('/').pop() : g;
      const m2 = matchDirPattern(leaf);
      if (m2) patternMatches[g] = m2;
      else if (g === 'root') patternMatches[g] = 'config';
      else if (g === 'src-tauri') patternMatches[g] = 'service';
      else if (g === 'src') patternMatches[g] = 'ui';
      else if (g === '.cursor') patternMatches[g] = 'documentation';
      else if (g === '.ua') patternMatches[g] = 'config';
    }
  }
  const finePatternMatches = {};
  for (const g of Object.keys(fineGroups)) {
    const leaf = g.includes('/') ? g.split('/').pop() : g;
    const m = matchDirPattern(leaf) || matchDirPattern(g);
    if (m) finePatternMatches[g] = m;
    else if (g === 'root') finePatternMatches[g] = 'config';
    else if (g.startsWith('src-tauri')) finePatternMatches[g] = 'service';
    else if (g === 'src') finePatternMatches[g] = 'ui';
  }

  const filePatterns = {};
  for (const n of fileNodes) {
    const fp = n.filePath || '';
    const bn = path.posix.basename(fp.replace(/\\/g, '/'));
    const p = fileLevelPattern(fp, bn);
    if (p) filePatterns[n.id] = p;
  }

  // H. Deployment topology
  const infraFiles = [];
  let hasDockerfile = false;
  let hasCompose = false;
  let hasK8s = false;
  let hasTerraform = false;
  let hasCI = false;
  for (const n of fileNodes) {
    const fp = (n.filePath || '').replace(/\\/g, '/');
    const bn = path.posix.basename(fp);
    if (/^Dockerfile/i.test(bn)) {
      hasDockerfile = true;
      infraFiles.push(fp);
    }
    if (/^docker-compose/i.test(bn)) {
      hasCompose = true;
      infraFiles.push(fp);
    }
    if (/(\/|^)(k8s|kubernetes|helm|charts)\//i.test(fp) || /\.ya?ml$/i.test(bn) && /k8s/i.test(fp)) {
      hasK8s = true;
      infraFiles.push(fp);
    }
    if (/\.tf(vars)?$/i.test(bn)) {
      hasTerraform = true;
      infraFiles.push(fp);
    }
    if (/\.github\/workflows\//.test(fp) || bn === '.gitlab-ci.yml' || bn === 'Jenkinsfile') {
      hasCI = true;
      infraFiles.push(fp);
    }
    if (bn === 'tauri.conf.json' || bn === 'build.rs' || /capabilities\//.test(fp)) {
      infraFiles.push(fp);
    }
  }

  // I. Data pipeline
  const schemaFiles = [];
  const migrationFiles = [];
  const dataModelFiles = [];
  const apiHandlerFiles = [];
  for (const n of fileNodes) {
    const fp = (n.filePath || '').replace(/\\/g, '/');
    const bn = path.posix.basename(fp);
    const tags = n.tags || [];
    const summary = (n.summary || '').toLowerCase();
    if (/\.(sql|graphql|gql|proto|prisma)$/i.test(bn) || /schema/i.test(bn)) schemaFiles.push(fp);
    if (/migration/i.test(fp)) migrationFiles.push(fp);
    if (/models?\./i.test(bn) || tags.includes('model') || /\/db\./i.test(fp) || bn === 'db.rs' || bn === 'models.rs') {
      dataModelFiles.push(fp);
    }
    if (
      tags.includes('api-handler') ||
      tags.includes('api') ||
      bn === 'commands.rs' ||
      /commands\./i.test(bn) ||
      summary.includes('tauri command') ||
      summary.includes('invoke')
    ) {
      apiHandlerFiles.push(fp);
    }
  }

  // J. Doc coverage
  const groupsWithDocsList = [];
  const undocumentedGroups = [];
  for (const g of Object.keys(directoryGroups)) {
    const ids = directoryGroups[g];
    const hasDoc = ids.some((id) => {
      const n = fileNodes.find((x) => x.id === id);
      const fp = (n && n.filePath) || '';
      return /\.(md|rst|mdc)$/i.test(fp) || (n && n.type === 'document');
    });
    // also README in group path
    const hasReadme = fileNodes.some((n) => {
      const fp = (n.filePath || '').replace(/\\/g, '/');
      if (!/^readme\.(md|rst)$/i.test(path.posix.basename(fp))) return false;
      if (g === 'root') return !fp.includes('/');
      return fp.startsWith(g + '/') || fp === g;
    });
    if (hasDoc || hasReadme) groupsWithDocsList.push(g);
    else undocumentedGroups.push(g);
  }
  const totalGroups = Object.keys(directoryGroups).length;
  const docCoverage = {
    groupsWithDocs: groupsWithDocsList.length,
    totalGroups,
    coverageRatio: totalGroups ? groupsWithDocsList.length / totalGroups : 0,
    undocumentedGroups,
  };

  // K. Dependency direction
  const pairCounts = {};
  for (const { from, to, count } of interGroupImports) {
    const a = from < to ? from : to;
    const b = from < to ? to : from;
    const key = `${a}\0${b}`;
    if (!pairCounts[key]) pairCounts[key] = { a, b, ab: 0, ba: 0 };
    if (from === a && to === b) pairCounts[key].ab += count;
    else pairCounts[key].ba += count;
  }
  // rebuild properly
  const directed = {};
  for (const { from, to, count } of interGroupImports) {
    if (!directed[from]) directed[from] = {};
    directed[from][to] = (directed[from][to] || 0) + count;
  }
  const dependencyDirection = [];
  const seenPairs = new Set();
  for (const from of Object.keys(directed)) {
    for (const to of Object.keys(directed[from])) {
      const pair = [from, to].sort().join('\0');
      if (seenPairs.has(pair)) continue;
      seenPairs.add(pair);
      const fwd = (directed[from] && directed[from][to]) || 0;
      const rev = (directed[to] && directed[to][from]) || 0;
      if (fwd > rev) dependencyDirection.push({ dependent: from, dependsOn: to });
      else if (rev > fwd) dependencyDirection.push({ dependent: to, dependsOn: from });
      else {
        dependencyDirection.push({ dependent: from, dependsOn: to });
        dependencyDirection.push({ dependent: to, dependsOn: from });
      }
    }
  }

  const filesPerGroup = {};
  for (const [g, ids] of Object.entries(directoryGroups)) filesPerGroup[g] = ids.length;
  const nodeTypeCounts = {};
  for (const [t, ids] of Object.entries(nodeTypeGroups)) nodeTypeCounts[t] = ids.length;

  const result = {
    scriptCompleted: true,
    directoryGroups,
    fineGroups,
    nodeTypeGroups,
    crossCategoryEdges,
    interGroupImports,
    intraGroupDensity,
    patternMatches,
    finePatternMatches,
    filePatterns,
    deploymentTopology: {
      hasDockerfile,
      hasCompose,
      hasK8s,
      hasTerraform,
      hasCI,
      infraFiles: [...new Set(infraFiles)],
    },
    dataPipeline: {
      schemaFiles,
      migrationFiles,
      dataModelFiles,
      apiHandlerFiles,
    },
    docCoverage,
    dependencyDirection,
    fileStats: {
      totalFileNodes: fileNodes.length,
      filesPerGroup,
      nodeTypeCounts,
    },
    fileFanIn: fanIn,
    fileFanOut: fanOut,
    commonPrefix: prefix,
  };

  try {
    fs.writeFileSync(outputPath, JSON.stringify(result, null, 2));
  } catch (e) {
    fail(`Failed to write output: ${e.message}`);
  }
  console.log(`OK: ${fileNodes.length} nodes -> ${outputPath}`);
}

main();
