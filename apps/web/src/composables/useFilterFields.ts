import { useQuery } from '@tanstack/vue-query'
import { computed, toValue, type MaybeRefOrGetter } from 'vue'

import { queryKeys } from '~/api/query-keys'
import { explorationsResource } from '~/api/resources/explorations'
import type { Condition, Field, Operator, PropertyType, Value } from '~/generated'
import { dimensionFields, integerFields, typeLabels } from '~/lib/exploration-labels'
import { useI18n } from '~/plugins/i18n'
import { canonical } from '~/services/exploration'

import { useExplorationFormat } from './useExplorationFormat'

/** How a field is compared: built-in text, built-in integer, or one property value type. */
export type FilterKind = 'text' | 'integer' | 'string' | 'number' | 'date' | 'bool' | 'array' | 'null'

export interface FilterField {
  /** Unique per field and kind: a property with two types is two entries. */
  key: string
  field: Field
  label: string
  kind: FilterKind
}

/** Operators by kind (0004 Operator by value type). Negations sit next to their positive form. */
export const operatorsFor: Record<FilterKind, Operator[]> = {
  text: ['eq', 'neq', 'contains', 'not_contains', 'starts_with', 'is_set', 'is_not_set'],
  integer: ['eq', 'neq', 'gt', 'gte', 'lt', 'lte'],
  string: ['eq', 'neq', 'contains', 'not_contains', 'starts_with', 'is_set', 'is_not_set', 'is_null'],
  number: ['eq', 'neq', 'gt', 'gte', 'lt', 'lte', 'is_set', 'is_not_set', 'is_null'],
  date: ['gt', 'gte', 'lt', 'lte', 'eq', 'neq', 'is_set', 'is_not_set'],
  bool: ['eq', 'is_set', 'is_not_set', 'is_null'],
  array: ['contains', 'not_contains', 'is_set', 'is_not_set'],
  null: ['is_null', 'is_set', 'is_not_set'],
}

export const valueless: Operator[] = ['is_set', 'is_not_set', 'is_null']

const kindOfType: Record<PropertyType, FilterKind> = {
  null: 'null',
  bool: 'bool',
  integer: 'number',
  decimal: 'number',
  date: 'date',
  string: 'string',
  array: 'array',
}

/** Parses typed text into the value a kind compares with; `null` when it does not parse. */
export function parseValue(kind: FilterKind, text: string): Value | null {
  const trimmed = text.trim()
  if (trimmed === '') return null
  switch (kind) {
    case 'integer':
      return /^-?\d+$/.test(trimmed) ? { t: 'integer', v: Number(trimmed) } : null
    case 'number':
      if (/^-?\d+$/.test(trimmed)) return { t: 'integer', v: Number(trimmed) }
      return /^-?\d+\.\d{1,4}$/.test(trimmed) ? { t: 'decimal', v: trimmed } : null
    case 'date':
      return /^\d{4}-\d{2}-\d{2}$/.test(trimmed) ? { t: 'date', v: `${trimmed}T00:00:00.000Z` } : null
    case 'bool':
      return trimmed === 'true' || trimmed === 'false' ? { t: 'bool', v: trimmed === 'true' } : null
    default:
      return { t: 'string', v: text }
  }
}

/** A value as the text it was typed as. */
export function valueText(value: Value): string {
  switch (value.t) {
    case 'date':
      return value.v.slice(0, 10)
    case 'list':
      return value.v.map(valueText).join(', ')
    default:
      return String(value.v)
  }
}

/** The kind a stored condition is edited as. */
export function kindOf(condition: Condition): FilterKind {
  const { field, value } = condition
  if (typeof field === 'string') return integerFields.includes(field) ? 'integer' : 'text'
  switch (value?.t) {
    case 'integer':
    case 'decimal':
      return 'number'
    case 'date':
      return 'date'
    case 'bool':
      return 'bool'
    default:
      return condition.cmp === 'is_null' ? 'null' : 'string'
  }
}

/** Fields a filter can use on a site: the built-in ones, then its property keys per stored type. */
export function useFilterFields(site: MaybeRefOrGetter<string>) {
  const { t } = useI18n()
  const { fieldLabel } = useExplorationFormat()
  const catalog = useQuery({
    queryKey: computed(() => queryKeys.catalog(toValue(site))),
    queryFn: ({ signal }) => explorationsResource.catalog(toValue(site), signal),
    staleTime: 5 * 60 * 1000,
  })

  const fields = computed<FilterField[]>(() => {
    const builtIn = dimensionFields.map<FilterField>((field) => ({
      key: field,
      field,
      label: fieldLabel(field),
      kind: integerFields.includes(field) ? 'integer' : 'text',
    }))
    const properties = (catalog.data.value?.properties ?? []).flatMap((property) => {
      const field: Field = { scope: property.scope, key: property.key }
      const kinds = [...new Set(property.types.map((type) => kindOfType[type]))]
      return kinds.map<FilterField>((kind) => {
        const type = property.types.find((candidate) => kindOfType[candidate] === kind) ?? 'string'
        return {
          key: `${canonical(field)}:${kind}`,
          field,
          kind,
          label:
            kinds.length > 1
              ? t('explore.propertyType', { key: fieldLabel(field), type: t(typeLabels[type]) })
              : fieldLabel(field),
        }
      })
    })
    return [...builtIn, ...properties]
  })

  return { fields, catalog }
}
