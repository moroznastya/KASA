// ============================================================================
// Kasa POS — ESLint Flat Config (v9+)
// ============================================================================
// Використання: npx eslint .
// ============================================================================
// @ts-check

import js from '@eslint/js';
import tseslint from 'typescript-eslint';
import reactHooks from 'eslint-plugin-react-hooks';
import reactRefresh from 'eslint-plugin-react-refresh';
import globals from 'globals';

export default tseslint.config(
    // Глобальне ігнорування
    { ignores: ['dist/', 'node_modules/', 'src-tauri/', 'build/'] },

    // Базові рекомендовані правила
    js.configs.recommended,

    // TypeScript рекомендовані правила
    ...tseslint.configs.recommended,

    // React Hooks + Refresh
    {
        plugins: {
            'react-hooks': reactHooks,
            'react-refresh': reactRefresh,
        },
        rules: {
            // React Hooks правила
            ...reactHooks.configs.recommended.rules,

            // React Refresh — дозволяємо експорт компонентів
            'react-refresh/only-export-components': [
                'warn',
                { allowConstantExport: true },
            ],

            // Відключаємо надто суворі правила
            '@typescript-eslint/no-unused-vars': [
                'warn',
                {
                    argsIgnorePattern: '^_',
                    varsIgnorePattern: '^_',
                },
            ],
            '@typescript-eslint/no-explicit-any': 'warn',

            // Змінні мають бути використані
            'no-unused-vars': 'off', // використовуємо TS версію
        },
    },

    // Специфічні налаштування для файлів
    {
        files: ['**/*.ts', '**/*.tsx'],
        languageOptions: {
            parser: tseslint.parser,
            parserOptions: {
                ecmaVersion: 2020,
                sourceType: 'module',
                ecmaFeatures: {
                    jsx: true,
                },
            },
            globals: {
                ...globals.browser,
                ...globals.es2020,
            },
        },
    },

    // Налаштування для конфігураційних файлів (vite.config.ts, postcss.config.js)
    {
        files: ['*.config.ts', '*.config.js', '*.config.mjs'],
        languageOptions: {
            globals: {
                ...globals.node,
            },
        },
        rules: {
            '@typescript-eslint/no-require-imports': 'off',
            'no-undef': 'off',
        },
    }
);
