import js from '@eslint/js'
import tseslint from 'typescript-eslint'
import eslintPluginReact from 'eslint-plugin-react'
import eslintPluginReactHooks from 'eslint-plugin-react-hooks'
import eslintPluginReactRefresh from 'eslint-plugin-react-refresh'

export default [
  { ignores: ['**/node_modules', '**/dist', '**/out', '**/src-tauri/target'] },
  js.configs.recommended,
  ...tseslint.configs.recommended,
  eslintPluginReact.configs.flat.recommended,
  eslintPluginReact.configs.flat['jsx-runtime'],
  {
    files: ['**/*.mjs'],
    languageOptions: {
      globals: {
        console: 'readonly',
        process: 'readonly',
        URL: 'readonly',
        Buffer: 'readonly',
        setTimeout: 'readonly',
        clearTimeout: 'readonly',
        setInterval: 'readonly',
        clearInterval: 'readonly',
        queueMicrotask: 'readonly',
        module: 'readonly',
        require: 'readonly'
      }
    }
  },
  {
    settings: {
      react: {
        version: 'detect'
      }
    }
  },
  {
    files: ['e2e-web/**/*.js'],
    languageOptions: {
      globals: {
        window: 'readonly',
        document: 'readonly',
        console: 'readonly'
      }
    }
  },
  {
    files: ['**/*.{ts,tsx}'],
    plugins: {
      'react-hooks': eslintPluginReactHooks,
      'react-refresh': eslintPluginReactRefresh
    },
    rules: {
      ...eslintPluginReactHooks.configs.recommended.rules,
      'react-refresh/only-export-components': ['warn', { allowConstantExport: true }]
    }
  },
  {
    // A failing node:assert comparison deep-inspects its operands; a jsdom node
    // reaches the whole window graph and the diff eats tens of GB. See
    // tests/render/assert-dom.ts.
    files: ['tests/render/**/*.{ts,tsx}'],
    rules: {
      'no-restricted-syntax': [
        'error',
        {
          selector:
            "CallExpression[callee.object.name='assert'][callee.property.name=/^(equal|strictEqual|notEqual|notStrictEqual|deepEqual|deepStrictEqual)$/] > CallExpression:first-child[callee.property.name=/^(query|get|find)(All)?By|^getElementById$|^querySelector(All)?$|^closest$/]",
          message:
            'Do not pass DOM nodes to assert.equal/deepEqual: a failure deep-inspects jsdom and exhausts memory. Use assertAbsent() from ./assert-dom.ts or compare a primitive.'
        },
        {
          selector:
            "CallExpression[callee.object.name='assert'][callee.property.name=/^(equal|strictEqual|notEqual|notStrictEqual|deepEqual|deepStrictEqual)$/] > MemberExpression:first-child[property.name='activeElement']",
          message:
            'Compare focus with assert.ok(document.activeElement === el): a failing assert.equal deep-inspects jsdom and exhausts memory.'
        }
      ]
    }
  }
]
